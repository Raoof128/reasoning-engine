//! `~/.reasoning-engine/config.toml`, with environment overrides.
//!
//! Precedence, highest first: environment variables, the config file,
//! defaults. Only `setup` writes the file; everything else just reads it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Default local HTTP port (plan section 3.3).
pub const DEFAULT_HTTP_PORT: u16 = 8765;

/// The local HTTP server (`serve`, behind the `local-host` feature).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    /// Whether `serve` may start without `--http`.
    pub enabled: bool,
    /// Bind address; loopback unless `--unsafe-bind-public`.
    pub host: String,
    /// Port.
    pub port: u16,
    /// MCP endpoint path.
    pub path: String,
    /// File holding `REASONING_ENGINE_HTTP_TOKEN=...` (mode 0600).
    pub token_file: Option<PathBuf>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".into(),
            port: DEFAULT_HTTP_PORT,
            path: "/mcp".into(),
            token_file: None,
        }
    }
}

/// An on/off switch section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Toggle {
    /// Whether the feature is on.
    pub enabled: bool,
}

/// Everything `config.toml` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Database, run packs and secrets live here.
    pub data_dir: PathBuf,
    /// Verifier tier (`lexical` until Phase 5 adds more).
    pub verifier: String,
    /// Local HTTP server.
    pub http: HttpConfig,
    /// Local audit UI.
    pub ui: Toggle,
    /// Cloudflare tunnel.
    pub tunnel: Toggle,
}

/// The user's home directory, if the platform reports one.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// `~/.reasoning-engine`, or `.reasoning-engine` when there is no home.
#[must_use]
pub fn default_data_dir() -> PathBuf {
    home_dir().unwrap_or_default().join(".reasoning-engine")
}

/// Expand a leading `~/` like Python's `Path.expanduser`.
#[must_use]
pub fn expand_user(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            verifier: "lexical".into(),
            http: HttpConfig::default(),
            ui: Toggle::default(),
            tunnel: Toggle::default(),
        }
    }
}

/// A non-empty path from the environment, with `~` expanded.
fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(|v| expand_user(Path::new(&v)))
}

/// `REASONING_ENGINE_CONFIG`, or `~/.reasoning-engine/config.toml`.
#[must_use]
pub fn config_path() -> PathBuf {
    env_path("REASONING_ENGINE_CONFIG").unwrap_or_else(|| default_data_dir().join("config.toml"))
}

/// Why a config file could not be used.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Reading or writing failed.
    #[error("config io error: {0}")]
    Io(#[from] std::io::Error),
    /// The file is not valid TOML for [`Config`].
    #[error("invalid config: {0}")]
    Parse(String),
}

impl Config {
    /// Read the config file. `Ok(None)` when it does not exist.
    ///
    /// # Errors
    /// The file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Option<Self>, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map(Some)
                .map_err(|e| ConfigError::Parse(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ConfigError::Io(e)),
        }
    }

    /// Write the config file, creating its directory.
    ///
    /// # Errors
    /// Filesystem failures.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| ConfigError::Parse(e.to_string()))?;
        std::fs::write(path, text).map_err(ConfigError::Io)
    }

    /// The data directory with `~` expanded.
    #[must_use]
    pub fn data_dir(&self) -> PathBuf {
        expand_user(&self.data_dir)
    }

    /// `REASONING_ENGINE_DB`, or `<data_dir>/reasoning.db`. Empty counts
    /// as unset, as with Python's `db_path or DEFAULT_DB_PATH`.
    #[must_use]
    pub fn db_path(&self) -> PathBuf {
        env_path("REASONING_ENGINE_DB").unwrap_or_else(|| self.data_dir().join("reasoning.db"))
    }

    /// `REASONING_ENGINE_RUNS_DIR`, or `<data_dir>/runs`. Empty counts as
    /// unset.
    #[must_use]
    pub fn runs_dir(&self) -> PathBuf {
        env_path("REASONING_ENGINE_RUNS_DIR").unwrap_or_else(|| self.data_dir().join("runs"))
    }

    /// `REASONING_ENGINE_VERIFIER`, or the configured tier.
    #[must_use]
    pub fn verifier(&self) -> String {
        std::env::var("REASONING_ENGINE_VERIFIER")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| self.verifier.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_fills_missing_sections() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("config.toml");
        assert_eq!(Config::load(&path).expect("load"), None);
        let mut config = Config::default();
        config.http.enabled = true;
        config.http.port = 9000;
        config.save(&path).expect("save");
        assert_eq!(Config::load(&path).expect("load"), Some(config));

        std::fs::write(&path, "verifier = \"lexical\"\n[http]\nport = 1234\n").expect("write");
        let partial = Config::load(&path).expect("load").expect("some");
        assert_eq!(
            (
                partial.http.port,
                partial.http.host.as_str(),
                partial.ui.enabled
            ),
            (1234, "127.0.0.1", false)
        );

        std::fs::write(&path, "http = 5").expect("write");
        assert!(matches!(Config::load(&path), Err(ConfigError::Parse(_))));
    }
}
