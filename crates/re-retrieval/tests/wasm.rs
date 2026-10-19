//! Sandboxed WASM source adapters (plan Phase 12): the example runs, a
//! declared host is fetched (from a replay tape), an undeclared host is
//! refused, and fuel, memory and imports are all bounded.
#![cfg(feature = "wasm-adapters")]

use std::path::Path;
use std::sync::Arc;

use re_retrieval::sources::SourceAdapter;
use re_retrieval::tape::{Outcome, Recording, Tape, redact_url, request_key};
use re_retrieval::wasm::{WasmAdapter, load};
use re_retrieval::{AcademicSearchAggregator, HttpClient};

fn example() -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/adapters/hello-adapter/hello.wat"),
    )
    .unwrap()
}

/// An adapter declaring `example.org` whose `search` is `body`; offset 0
/// holds the hosts and 512 the string `data`.
fn adapter(data: &str, body: &str) -> String {
    let hosts = r#"["example.org"]"#;
    let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        r#"(module
  (import "host" "http_get" (func $http_get (param i32 i32) (result i64)))
  (memory (export "memory") 1)
  (global $next (mut i32) (i32.const 8192))
  (func (export "alloc") (param $len i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (if (i32.gt_u (global.get $next) (i32.mul (memory.size) (i32.const 65536)))
      (then (drop (memory.grow (i32.add (i32.const 1)
        (i32.div_u (global.get $next) (i32.const 65536)))))))
    (local.get $ptr))
  (data (i32.const 0) "{}")
  (func (export "allowed_hosts") (result i64) (i64.const {}))
  (data (i32.const 512) "{}")
  (data (i32.const 2048) "{{\"error\":\"fetch refused\"}}")
  (func (export "search") (param i32 i32 i32) (result i64)
    (local $got i64)
    {body})
)"#,
        escape(hosts),
        hosts.len(),
        escape(data),
    )
}

/// Fetch the URL at 512 (`len` bytes) and return the body, or the error at
/// 2048 when the host refuses.
fn fetching(url: &str) -> String {
    adapter(
        url,
        &format!(
            "(local.set $got (call $http_get (i32.const 512) (i32.const {})))
    (if (result i64) (i64.lt_s (local.get $got) (i64.const 0))
      (then (i64.or (i64.shl (i64.const 2048) (i64.const 32)) (i64.const 25)))
      (else (local.get $got)))",
            url.len()
        ),
    )
}

fn client() -> HttpClient {
    HttpClient::new().unwrap()
}

#[tokio::test]
async fn the_example_adapter_runs() {
    let hello = WasmAdapter::new("hello", &example(), client()).unwrap();
    assert_eq!(hello.allowed_hosts(), ["example.org"]);
    let result = hello.search("run", "anything", 5).await;
    assert_eq!(result.error, None);
    assert_eq!(result.evidence.len(), 1);
    let record = &result.evidence[0];
    assert_eq!(record.title, "Hello from WASM");
    assert_eq!(record.source_adapter, "hello");
    assert_eq!(record.year, Some(2026));
    assert_eq!(record.url.as_deref(), Some("https://example.org/hello"));
}

#[tokio::test]
async fn declared_hosts_are_fetched_through_the_tape() {
    let dir = tempfile::tempdir().unwrap();
    let url = "https://example.org/api";
    let body = r#"[{"title":"Fetched","snippet":"From the host.","doi":"10.1/w"}]"#;
    let recording = Recording {
        method: "GET".into(),
        url: redact_url(url),
        outcome: Outcome::Response {
            status: 200,
            headers: std::collections::BTreeMap::default(),
            body: Some(body.into()),
            body_base64: None,
        },
    };
    std::fs::write(
        dir.path().join(format!("{}.json", request_key("GET", url))),
        serde_json::to_string(&recording).unwrap(),
    )
    .unwrap();
    let tape = Arc::new(Tape::replaying(dir.path()).unwrap());
    let wasm = WasmAdapter::new(
        "fetcher",
        fetching(url).as_bytes(),
        client().with_tape(tape),
    )
    .unwrap();
    let result = wasm.search("run", "q", 5).await;
    assert_eq!(result.error, None, "{:?}", result.error);
    assert_eq!(result.evidence[0].title, "Fetched");
    assert_eq!(result.evidence[0].doi.as_deref(), Some("10.1/w"));
}

#[tokio::test]
async fn undeclared_and_private_hosts_are_refused() {
    for url in [
        "https://evil.example.com/steal",
        "http://example.org/plain-http",
        "https://169.254.169.254/latest/meta-data",
    ] {
        let wasm = WasmAdapter::new("nosy", fetching(url).as_bytes(), client()).unwrap();
        let result = wasm.search("run", "q", 5).await;
        let error = result.error.expect("refused");
        assert!(error.message.contains("fetch refused"), "{url}: {error:?}");
        assert_eq!(result.evidence, []);
    }
}

#[tokio::test]
async fn fuel_memory_and_imports_are_bounded() {
    let spin = adapter("", "(loop $forever (br $forever)) (i64.const 0)");
    let result = WasmAdapter::new("spin", spin.as_bytes(), client())
        .unwrap()
        .search("run", "q", 5)
        .await;
    let error = result.error.expect("cut off");
    assert!(error.message.contains("ran out of fuel"), "{error:?}");

    // Growing past 64 MiB fails inside the adapter (memory.grow returns -1).
    let hog = adapter(
        "",
        "(if (result i64) (i32.lt_s (memory.grow (i32.const 2000)) (i32.const 0))
      (then (i64.or (i64.shl (i64.const 2048) (i64.const 32)) (i64.const 25)))
      (else (i64.const 0)))",
    );
    let result = WasmAdapter::new("hog", hog.as_bytes(), client())
        .unwrap()
        .search("run", "q", 5)
        .await;
    assert!(
        result
            .error
            .expect("capped")
            .message
            .contains("fetch refused"),
        "the grow was refused"
    );

    let wasi = r#"(module
  (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1))"#;
    let error = WasmAdapter::new("wasi", wasi.as_bytes(), client()).unwrap_err();
    assert!(
        error.message.contains("does not provide"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn adapters_are_loaded_by_name_and_aggregated() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.wat"), example()).unwrap();
    std::fs::write(dir.path().join("broken.wasm"), b"not wasm").unwrap();
    let loaded = load(dir.path(), &["hello", "broken", "absent"], &client());
    assert_eq!(loaded.len(), 1, "broken and absent are skipped");
    let adapters: Vec<re_retrieval::aggregate::Adapter> = loaded
        .into_iter()
        .map(|a| Arc::new(a) as re_retrieval::aggregate::Adapter)
        .collect();
    let result = AcademicSearchAggregator::new(adapters)
        .search("run", "anything", 5)
        .await;
    assert_eq!(result.evidence[0].title, "Hello from WASM");
}

#[test]
fn unknown_sources_are_looked_up_in_the_adapters_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.wat"), example()).unwrap();
    let home = dir.path().to_string_lossy().into_owned();
    let env = |var: &str| match var {
        "REASONING_ENGINE_SOURCES" => Some("openalex, hello, nowhere".to_string()),
        "REASONING_ENGINE_ADAPTERS_DIR" => Some(home.clone()),
        _ => None,
    };
    let names: Vec<&str> = re_retrieval::aggregate::adapters_from(env)
        .unwrap()
        .iter()
        .map(|a| a.name())
        .collect();
    assert_eq!(names, ["openalex", "hello"]);
}
