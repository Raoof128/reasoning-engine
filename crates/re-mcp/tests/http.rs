//! The local Streamable HTTP server (mirrors `tests/test_http_transport.py`
//! and covers the port's extra hardening).
#![cfg(feature = "local-host")]

use std::net::SocketAddr;
use std::sync::Arc;

use re_mcp::http::{HttpOptions, MAX_REQUEST_BODY_BYTES, serve};
use re_mcp::{ReasoningServer, Tools};
use re_retrieval::AcademicSearchAggregator;
use re_retrieval::sources::mock::MockScholarGateway;
use re_store::Store;
use re_verify::{LexicalOverlap, VerifiableResearchService};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HOST, ORIGIN};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;

struct Running {
    addr: SocketAddr,
    shutdown: CancellationToken,
    _dir: tempfile::TempDir,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

async fn start(token: Option<&str>) -> Running {
    let dir = tempfile::tempdir().expect("tmp");
    let store = Store::open(dir.path().join("reasoning.db")).expect("store");
    let search = Arc::new(AcademicSearchAggregator::new(vec![Arc::new(
        MockScholarGateway,
    )]));
    let service = VerifiableResearchService::new(
        store,
        dir.path().join("runs"),
        search,
        Box::new(LexicalOverlap),
    );
    let server = ReasoningServer::new(Tools::new(Arc::new(service)));
    let options = HttpOptions {
        host: "127.0.0.1".into(),
        port: 0,
        path: "/mcp".into(),
        bearer_token: token.map(Into::into),
        unsafe_bind_public: false,
    };
    let shutdown = CancellationToken::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let stop = shutdown.clone();
    tokio::spawn(async move {
        serve(server, &options, stop, |addr| {
            let _ = tx.send(addr);
        })
        .await
        .expect("serves");
    });
    Running {
        addr: rx.await.expect("bound"),
        shutdown,
        _dir: dir,
    }
}

fn post(running: &Running, body: impl Into<reqwest::Body>) -> reqwest::RequestBuilder {
    reqwest::Client::new()
        .post(format!("http://{}/mcp", running.addr))
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .body(body)
}

/// The JSON-RPC message in a JSON or single-event SSE response body.
fn message(body: &str) -> Value {
    let json = body
        .lines()
        .find_map(|l| {
            l.strip_prefix("data:")
                .map(str::trim)
                .filter(|d| d.starts_with('{'))
        })
        .unwrap_or(body);
    serde_json::from_str(json).expect("JSON-RPC message")
}

#[tokio::test]
async fn loopback_clients_can_initialize() {
    let running = start(None).await;
    let response = post(&running, INITIALIZE).send().await.expect("send");
    assert_eq!(response.status(), 200);
    let reply = message(&response.text().await.expect("body"));
    assert_eq!(reply["result"]["serverInfo"]["name"], "reasoning-engine");
}

#[tokio::test]
async fn bearer_token_is_required_when_configured() {
    let running = start(Some("s3cret-token")).await;
    let missing = post(&running, INITIALIZE).send().await.expect("send");
    assert_eq!(missing.status(), 401);
    assert_eq!(missing.headers()["www-authenticate"], "Bearer");
    let wrong = post(&running, INITIALIZE)
        .header(AUTHORIZATION, "Bearer nope")
        .send()
        .await
        .expect("send");
    assert_eq!(wrong.status(), 401);
    let prefix = post(&running, INITIALIZE)
        .header(AUTHORIZATION, "Bearer s3cret")
        .send()
        .await
        .expect("send");
    assert_eq!(
        prefix.status(),
        401,
        "a prefix of the token is not the token"
    );
    let valid = post(&running, INITIALIZE)
        .header(AUTHORIZATION, "Bearer s3cret-token")
        .send()
        .await
        .expect("send");
    assert_eq!(valid.status(), 200);
}

#[tokio::test]
async fn foreign_hosts_and_browser_origins_are_refused() {
    let running = start(None).await;
    let rebinding = post(&running, INITIALIZE)
        .header(HOST, "evil.example")
        .send()
        .await
        .expect("send");
    assert!(
        matches!(rebinding.status().as_u16(), 403 | 421),
        "{}",
        rebinding.status()
    );
    let browser = post(&running, INITIALIZE)
        .header(ORIGIN, "https://evil.example")
        .send()
        .await
        .expect("send");
    assert_eq!(browser.status(), 403);
    let local_page = post(&running, INITIALIZE)
        .header(ORIGIN, "http://localhost:3000")
        .send()
        .await
        .expect("send");
    assert_eq!(
        local_page.status(),
        403,
        "no page may drive the tools from a browser"
    );
}

#[tokio::test]
async fn oversized_bodies_are_rejected() {
    let running = start(None).await;
    let padding = "x".repeat(MAX_REQUEST_BODY_BYTES);
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "ping", "params": {"pad": padding}})
        .to_string();
    let response = post(&running, body).send().await.expect("send");
    assert_eq!(response.status(), 413);
}

#[tokio::test]
async fn tools_are_the_same_over_http_and_stdio() {
    let running = start(None).await;
    let init = post(&running, INITIALIZE).send().await.expect("send");
    let session = init.headers().get("mcp-session-id").cloned();
    let _ = init.text().await;
    let mut initialized = post(
        &running,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    )
    .header("mcp-protocol-version", "2025-06-18");
    let mut list = post(
        &running,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    )
    .header("mcp-protocol-version", "2025-06-18");
    if let Some(id) = session {
        initialized = initialized.header("mcp-session-id", id.clone());
        list = list.header("mcp-session-id", id);
    }
    let _ = initialized.send().await.expect("send");
    let response = list.send().await.expect("send");
    assert_eq!(response.status(), 200);
    let reply = message(&response.text().await.expect("body"));
    let names: Vec<&str> = reply["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|t| t["name"].as_str().expect("name"))
        .collect();
    let stdio: Vec<String> = ReasoningServer::tool_definitions()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(names, stdio);
    assert!(names.contains(&"run_research_pipeline_tool"));
}
