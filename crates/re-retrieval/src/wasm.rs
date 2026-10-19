//! Sandboxed third-party source adapters (`wasm-adapters` feature, plan
//! Phase 12).
//!
//! An adapter is a WebAssembly core module (`.wasm`, or `.wat` text) that
//! exports:
//!
//! - `memory`;
//! - `alloc(len: i32) -> i32`: room for `len` bytes the host may write;
//! - `allowed_hosts() -> i64`: a JSON array of the host names it may fetch;
//! - `search(query_ptr: i32, query_len: i32, limit: i32) -> i64`: a JSON
//!   array of records (`title`, `snippet`, `url`, `doi`, `year`,
//!   `authors`, `venue`, `score`), or `{"error": "..."}`.
//!
//! An `i64` result packs a pointer and a length: `ptr << 32 | len`.
//!
//! The host gives it exactly two imports from module `host` and nothing
//! else (no WASI: no files, no environment, no clock, no sockets):
//!
//! - `http_get(url_ptr: i32, url_len: i32) -> i64`: the response body,
//!   written through the adapter's `alloc`; `-1` when the URL's host was not
//!   declared (or is not public), `-2` when the request failed;
//! - `log(ptr: i32, len: i32)`: a line on stderr, prefixed with the
//!   adapter's name.
//!
//! Each search runs with [`FUEL`] units of fuel and a [`MEMORY_LIMIT`]
//! memory cap, so a runaway loop or allocation ends the search with an
//! error rather than the server. Fetches go through the engine's
//! [`HttpClient`], so they are recorded and replayed like any source.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use wasmtime::{
    Caller, Config, Engine, Extern, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
};

use crate::base::{RecordInput, build_record};
use crate::client::{HttpClient, is_public_url};
use crate::error::AdapterError;
use crate::result::RetrievalResult;
use crate::sources::{BoxFuture, SourceAdapter};

/// Fuel per search: enough for real parsing work, not for a runaway loop.
pub const FUEL: u64 = 10_000_000;
/// Linear memory an adapter may grow to.
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// Largest response body handed to an adapter.
pub const MAX_BODY: usize = 4 * 1024 * 1024;
/// Largest search result read back from an adapter.
pub const MAX_RESULT: usize = 4 * 1024 * 1024;

fn failure(detail: impl std::fmt::Display) -> AdapterError {
    AdapterError::new("adapter_misconfigured", detail.to_string(), false)
}

fn unpack(packed: i64) -> Option<(usize, usize)> {
    let packed = u64::try_from(packed).ok()?;
    let ptr = usize::try_from(packed >> 32).ok()?;
    let len = usize::try_from(packed & 0xffff_ffff).ok()?;
    Some((ptr, len))
}

fn pack(ptr: i32, len: usize) -> i64 {
    let ptr = i64::from(u32::from_ne_bytes(ptr.to_ne_bytes()));
    (ptr << 32) | i64::try_from(len).unwrap_or(0)
}

/// What a search's store carries.
struct State {
    name: String,
    allowed: Vec<String>,
    client: HttpClient,
    /// Present while searching; loading needs no network.
    runtime: Option<tokio::runtime::Handle>,
    limits: StoreLimits,
}

fn guest_bytes(caller: &mut Caller<'_, State>, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let memory = caller.get_export("memory").and_then(Extern::into_memory)?;
    let (ptr, len) = (usize::try_from(ptr).ok()?, usize::try_from(len).ok()?);
    memory
        .data(&caller)
        .get(ptr..ptr.checked_add(len)?)
        .map(<[u8]>::to_vec)
}

/// Copy `bytes` into the guest through its `alloc`; the packed location.
fn give(caller: &mut Caller<'_, State>, bytes: &[u8]) -> Option<i64> {
    let alloc = caller
        .get_export("alloc")
        .and_then(Extern::into_func)?
        .typed::<i32, i32>(&caller)
        .ok()?;
    let ptr = alloc
        .call(&mut *caller, i32::try_from(bytes.len()).ok()?)
        .ok()?;
    let memory = caller.get_export("memory").and_then(Extern::into_memory)?;
    memory
        .write(&mut *caller, usize::try_from(ptr).ok()?, bytes)
        .ok()?;
    Some(pack(ptr, bytes.len()))
}

fn http_get(mut caller: Caller<'_, State>, ptr: i32, len: i32) -> i64 {
    let Some(url) = guest_bytes(&mut caller, ptr, len).and_then(|b| String::from_utf8(b).ok())
    else {
        return -2;
    };
    let state = caller.data();
    let allowed = reqwest::Url::parse(&url).is_ok_and(|parsed| {
        is_public_url(&parsed)
            && parsed.scheme() == "https"
            && parsed
                .host_str()
                .is_some_and(|host| state.allowed.iter().any(|a| a.eq_ignore_ascii_case(host)))
    });
    if !allowed {
        eprintln!("[{}] refused a request to an undeclared host", state.name);
        return -1;
    }
    let Some(runtime) = &state.runtime else {
        return -2;
    };
    let fetched = runtime.block_on(
        state
            .client
            .get_document(&url, "application/json", MAX_BODY),
    );
    match fetched {
        Ok(fetched) => give(&mut caller, &fetched.body).unwrap_or(-2),
        Err(error) => {
            eprintln!("[{}] request failed: {}", caller.data().name, error.message);
            -2
        }
    }
}

fn log(mut caller: Caller<'_, State>, ptr: i32, len: i32) {
    if let Some(bytes) = guest_bytes(&mut caller, ptr, len.min(4096)) {
        eprintln!(
            "[{}] {}",
            caller.data().name,
            String::from_utf8_lossy(&bytes)
        );
    }
}

/// One record as an adapter reports it.
#[derive(Debug, Deserialize)]
struct Found {
    title: String,
    #[serde(default)]
    snippet: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    doi: Option<String>,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    authors: Vec<String>,
    #[serde(default)]
    venue: String,
    #[serde(default)]
    score: Option<f64>,
}

/// A loaded adapter. Cloning is cheap: the engine and compiled module are
/// shared.
#[derive(Clone)]
pub struct WasmAdapter {
    name: &'static str,
    engine: Engine,
    module: Module,
    allowed: Vec<String>,
    client: HttpClient,
}

impl std::fmt::Debug for WasmAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmAdapter")
            .field("name", &self.name)
            .field("allowed", &self.allowed)
            .finish_non_exhaustive()
    }
}

fn engine() -> Result<Engine, AdapterError> {
    let mut config = Config::new();
    config.consume_fuel(true);
    Engine::new(&config).map_err(failure)
}

impl WasmAdapter {
    /// Compile `bytes` (binary or text) as the adapter `name`, fetching
    /// through `client`, and read its declared hosts.
    ///
    /// # Errors
    /// A module that does not compile, lacks the exports, or imports
    /// anything but `host.http_get` and `host.log`.
    pub fn new(name: &str, bytes: &[u8], client: HttpClient) -> Result<Self, AdapterError> {
        let engine = engine()?;
        let module = Module::new(&engine, bytes).map_err(failure)?;
        for import in module.imports() {
            if import.module() != "host" || !matches!(import.name(), "http_get" | "log") {
                return Err(failure(format!(
                    "adapter {name} imports {}.{}, which the host does not provide",
                    import.module(),
                    import.name()
                )));
            }
        }
        // Leaked once per loaded adapter: adapters load at start-up and live
        // as long as the process.
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        let mut adapter = Self {
            name,
            engine,
            module,
            allowed: Vec::new(),
            client,
        };
        let declared = adapter.call_packed(None, |store, instance| {
            instance
                .get_typed_func::<(), i64>(&mut *store, "allowed_hosts")
                .map_err(failure)?
                .call(&mut *store, ())
                .map_err(failure)
        })?;
        adapter.allowed = serde_json::from_slice(&declared)
            .map_err(|e| failure(format!("adapter {name}: bad allowed_hosts: {e}")))?;
        Ok(adapter)
    }

    /// The hosts it may fetch.
    #[must_use]
    pub fn allowed_hosts(&self) -> &[String] {
        &self.allowed
    }

    /// Instantiate in a fresh, fuelled, memory-capped store, run `call`, and
    /// read back the packed bytes it returns.
    fn call_packed(
        &self,
        runtime: Option<&tokio::runtime::Handle>,
        call: impl FnOnce(&mut Store<State>, &wasmtime::Instance) -> Result<i64, AdapterError>,
    ) -> Result<Vec<u8>, AdapterError> {
        let state = State {
            name: self.name.to_string(),
            allowed: self.allowed.clone(),
            client: self.client.clone(),
            runtime: runtime.cloned(),
            limits: StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT).build(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
        store.set_fuel(FUEL).map_err(failure)?;
        let mut linker: Linker<State> = Linker::new(&self.engine);
        linker
            .func_wrap("host", "http_get", http_get)
            .map_err(failure)?;
        linker.func_wrap("host", "log", log).map_err(failure)?;
        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(failure)?;
        let packed = call(&mut store, &instance).map_err(|e| {
            let out_of_fuel = store.get_fuel().is_ok_and(|fuel| fuel == 0);
            if out_of_fuel {
                failure(format!("adapter {} ran out of fuel", self.name))
            } else {
                e
            }
        })?;
        let (ptr, len) = unpack(packed)
            .filter(|(_, len)| *len <= MAX_RESULT)
            .ok_or_else(|| failure(format!("adapter {} returned no result", self.name)))?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| failure(format!("adapter {} exports no memory", self.name)))?;
        memory
            .data(&store)
            .get(ptr..ptr + len)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| failure(format!("adapter {} returned out of bounds", self.name)))
    }

    fn run(
        &self,
        runtime: &tokio::runtime::Handle,
        run_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<RetrievalResult, AdapterError> {
        let bytes = self.call_packed(Some(runtime), |store, instance| {
            let alloc = instance
                .get_typed_func::<i32, i32>(&mut *store, "alloc")
                .map_err(failure)?;
            let len = i32::try_from(query.len()).map_err(failure)?;
            let ptr = alloc.call(&mut *store, len).map_err(failure)?;
            let memory = instance
                .get_memory(&mut *store, "memory")
                .ok_or_else(|| failure("no memory"))?;
            memory
                .write(
                    &mut *store,
                    usize::try_from(ptr).map_err(failure)?,
                    query.as_bytes(),
                )
                .map_err(failure)?;
            instance
                .get_typed_func::<(i32, i32, i32), i64>(&mut *store, "search")
                .map_err(failure)?
                .call(
                    &mut *store,
                    (ptr, len, i32::try_from(limit).unwrap_or(i32::MAX)),
                )
                .map_err(failure)
        })?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| failure(format!("adapter {}: bad result: {e}", self.name)))?;
        if let Some(message) = value.get("error").and_then(Value::as_str) {
            return Err(AdapterError::new(
                "unavailable",
                format!("adapter {}: {message}", self.name),
                true,
            ));
        }
        let found: Vec<Found> = serde_json::from_value(value)
            .map_err(|e| failure(format!("adapter {}: bad records: {e}", self.name)))?;
        let evidence = found
            .into_iter()
            .take(limit)
            .zip(1..)
            .map(|(f, rank)| {
                build_record(RecordInput {
                    title: f.title,
                    snippet: f.snippet,
                    url: f.url,
                    doi: f.doi.map_or(Value::Null, Value::from),
                    year: f.year,
                    authors: f.authors,
                    venue: f.venue,
                    score: f.score.unwrap_or(0.5).clamp(0.0, 1.0),
                    ..RecordInput::new(run_id, self.name, query, rank)
                })
            })
            .collect();
        Ok(RetrievalResult::ok(evidence))
    }
}

fn tokio_handle() -> Result<tokio::runtime::Handle, AdapterError> {
    tokio::runtime::Handle::try_current().map_err(|_| failure("WASM adapters need a Tokio runtime"))
}

impl SourceAdapter for WasmAdapter {
    fn name(&self) -> &'static str {
        self.name
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, RetrievalResult> {
        Box::pin(async move {
            let runtime = match tokio_handle() {
                Ok(runtime) => runtime,
                Err(error) => return RetrievalResult::failed(error.to_retrieval_error(self.name)),
            };
            // Compiled code and blocking fetches stay off the async workers.
            let adapter = self.clone();
            let (run_id, query) = (run_id.to_string(), query.to_string());
            let outcome =
                tokio::task::spawn_blocking(move || adapter.run(&runtime, &run_id, &query, limit))
                    .await
                    .unwrap_or_else(|e| Err(failure(format!("adapter task failed: {e}"))));
            match outcome {
                Ok(result) => result,
                Err(error) => RetrievalResult::failed(error.to_retrieval_error(self.name)),
            }
        })
    }
}

/// `<dir>/<name>.wasm` (or `.wat`) for each `name`, compiled; a name with
/// no file is skipped and a broken file is reported and skipped.
#[must_use]
pub fn load(dir: &Path, names: &[&str], client: &HttpClient) -> Vec<WasmAdapter> {
    names
        .iter()
        .filter_map(|name| {
            let path: PathBuf = ["wasm", "wat"]
                .iter()
                .map(|ext| dir.join(format!("{name}.{ext}")))
                .find(|p| p.is_file())?;
            let bytes = std::fs::read(&path).ok()?;
            match WasmAdapter::new(name, &bytes, client.clone()) {
                Ok(adapter) => Some(adapter),
                Err(error) => {
                    eprintln!(
                        "warning: skipping adapter {}: {}",
                        path.display(),
                        error.message
                    );
                    None
                }
            }
        })
        .collect()
}

/// Where adapters live: `REASONING_ENGINE_ADAPTERS_DIR`, or
/// `~/.reasoning-engine/adapters`.
#[must_use]
pub fn adapters_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(dir) = env("REASONING_ENGINE_ADAPTERS_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    env("HOME").or_else(|| env("USERPROFILE")).map(|home| {
        PathBuf::from(home)
            .join(".reasoning-engine")
            .join("adapters")
    })
}

/// Shared handle for callers that build adapters once.
pub type Shared = Arc<WasmAdapter>;
