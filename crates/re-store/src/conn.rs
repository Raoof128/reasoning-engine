//! Opening and hardening the database (port of `init_db` and `get_conn`).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use rusqlite::{Connection, Transaction};

use crate::error::StoreError;
use crate::schema::{PYTHON_USER_VERSION, SCHEMA_SQL};

/// A handle on one engine database.
///
/// Python opens a fresh connection per call; the port keeps one connection
/// behind a mutex and runs every public operation in its own transaction,
/// which preserves Python's one-commit-per-call semantics.
#[derive(Debug)]
pub struct Store {
    conn: Mutex<Connection>,
    path: PathBuf,
}

fn expand_user(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match (text, home) {
        ("~", Some(home)) => PathBuf::from(home),
        (rest, Some(home)) if rest.starts_with("~/") => PathBuf::from(home).join(&rest[2..]),
        _ => path.to_path_buf(),
    }
}

fn harden(conn: &Connection) -> Result<(), StoreError> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // Not in Python: refuse to run SQL functions embedded in a hostile schema.
    conn.pragma_update(None, "trusted_schema", "OFF")?;
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "same signature as the fallible unix version"
)]
fn restrict_permissions(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

impl Store {
    /// Open or create the database, apply the schema, and harden the
    /// connection. Idempotent, like Python's `init_db`.
    ///
    /// A new file gets mode `0600` on Unix. A `user_version` of 0 is set to
    /// Python's 2 so the file looks the same to both implementations; an
    /// existing value is never changed.
    ///
    /// # Errors
    /// I/O failures preparing the directory, or `SQLite` failures.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = expand_user(path.as_ref());
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let existed = path.exists();
        let conn = Connection::open(&path)?;
        harden(&conn)?;
        conn.execute_batch(SCHEMA_SQL)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version == 0 {
            conn.pragma_update(None, "user_version", PYTHON_USER_VERSION)?;
        }
        if !existed {
            restrict_permissions(&path)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// The resolved database path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run `f` in a transaction; commit on success, roll back on error.
    pub(crate) fn with_tx<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let tx = conn.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    /// Names of all tables, sorted.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn table_names(&self) -> Result<Vec<String>, StoreError> {
        self.with_tx(|tx| {
            let mut stmt =
                tx.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
            let names = stmt
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            Ok(names)
        })
    }

    /// `PRAGMA user_version`.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn user_version(&self) -> Result<i64, StoreError> {
        self.with_tx(|tx| Ok(tx.query_row("PRAGMA user_version", [], |row| row.get(0))?))
    }

    /// Rows in `table`, for diagnostics and tests. `table` must be one of
    /// [`crate::schema::TABLES`].
    ///
    /// # Errors
    /// [`StoreError::Invalid`] for an unknown table, or `SQLite` failures.
    pub fn count_rows(&self, table: &str) -> Result<i64, StoreError> {
        if !crate::schema::TABLES.contains(&table) {
            return Err(StoreError::invalid(format!("unknown table: {table}")));
        }
        self.with_tx(|tx| {
            Ok(
                tx.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })?,
            )
        })
    }
}
