//! Opt-in local Streamable HTTP server (`local-host` feature; port of the
//! HTTP half of `transport.py`).
//!
//! Defence in depth for a server on a laptop:
//!
//! - binds loopback unless `unsafe_bind_public`, and a public bind also
//!   needs a bearer token (Python allows tokenless public binds);
//! - only loopback `Host` headers are accepted on a loopback bind (DNS
//!   rebinding), and any request carrying an `Origin` header is refused, so
//!   no web page can drive the tools from a browser;
//! - bearer tokens are compared in constant time and never logged;
//! - request bodies are capped at 1 MiB.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use subtle::ConstantTimeEq;
use tokio_util::sync::CancellationToken;

use crate::server::ReasoningServer;

/// Request body cap.
pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
/// Hosts that count as loopback.
pub const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "::1"];

/// Where and how to serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpOptions {
    /// Bind address.
    pub host: String,
    /// Port; 0 picks a free one (tests).
    pub port: u16,
    /// MCP endpoint path, e.g. `/mcp`.
    pub path: String,
    /// Required bearer token, if any.
    pub bearer_token: Option<String>,
    /// Allow a non-loopback bind.
    pub unsafe_bind_public: bool,
}

fn is_loopback(host: &str) -> bool {
    LOOPBACK_HOSTS.contains(&host) || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// `validate_http_bind`, stricter: every non-loopback host is public (Python
/// only treats `0.0.0.0`, `::` and the empty string as public), and a public
/// bind needs a bearer token.
///
/// # Errors
/// Python's messages for a bad port or an unflagged public bind, plus one
/// for a public bind without a token.
pub fn validate_http_bind(options: &HttpOptions) -> Result<String, String> {
    let host = re_core::pystr::py_strip(&options.host).to_string();
    if options.port == 0 && !is_loopback(&host) {
        return Err("port must be between 1 and 65535".into());
    }
    if !is_loopback(&host) {
        if !options.unsafe_bind_public {
            return Err("public HTTP bind requires --unsafe-bind-public".into());
        }
        if options.bearer_token.as_deref().is_none_or(str::is_empty) {
            return Err("public HTTP bind requires a bearer token".into());
        }
    }
    if !options.path.starts_with('/') {
        return Err("path must start with /".into());
    }
    Ok(host)
}

async fn require_bearer(State(token): State<Arc<str>>, request: Request, next: Next) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    if bool::from(presented.as_bytes().ct_eq(token.as_bytes())) {
        return next.run(request).await;
    }
    let mut response = StatusCode::UNAUTHORIZED.into_response();
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

/// The axum router serving `server` at `options.path`.
pub fn router(
    server: ReasoningServer,
    options: &HttpOptions,
    shutdown: CancellationToken,
) -> Router {
    let host = re_core::pystr::py_strip(&options.host);
    let mut config = StreamableHttpServerConfig::default()
        .with_max_request_body_bytes(MAX_REQUEST_BODY_BYTES)
        .with_cancellation_token(shutdown)
        .enforce_origin_validation();
    config = if is_loopback(host) {
        config.with_allowed_hosts(LOOPBACK_HOSTS)
    } else {
        // Clients reach a public bind by names we cannot know; the
        // mandatory bearer token is the guard there.
        config.disable_allowed_hosts()
    };
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let router = Router::new().nest_service(&options.path, service);
    match options.bearer_token.as_deref().filter(|t| !t.is_empty()) {
        Some(token) => router.layer(middleware::from_fn_with_state(
            Arc::<str>::from(token),
            require_bearer,
        )),
        None => router,
    }
}

/// Bind and serve until `shutdown` is cancelled. Returns the bound address
/// through `on_bound` before serving, so callers and tests can connect.
///
/// # Errors
/// An invalid bind or a socket failure.
pub async fn serve(
    server: ReasoningServer,
    options: &HttpOptions,
    shutdown: CancellationToken,
    on_bound: impl FnOnce(SocketAddr),
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let host = validate_http_bind(options)?;
    let listener = tokio::net::TcpListener::bind((host.as_str(), options.port)).await?;
    on_bound(listener.local_addr()?);
    let app = router(server, options, shutdown.clone());
    axum::serve(listener, app)
        .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(host: &str, unsafe_bind_public: bool, token: Option<&str>) -> HttpOptions {
        HttpOptions {
            host: host.into(),
            port: 8765,
            path: "/mcp".into(),
            bearer_token: token.map(Into::into),
            unsafe_bind_public,
        }
    }

    #[test]
    fn binds_follow_python_and_are_stricter() {
        assert_eq!(
            validate_http_bind(&options(" 127.0.0.1 ", false, None)),
            Ok("127.0.0.1".into())
        );
        assert_eq!(
            validate_http_bind(&options("::1", false, None)),
            Ok("::1".into())
        );
        for public in ["0.0.0.0", "::", "", "192.168.1.5", "example.org"] {
            assert_eq!(
                validate_http_bind(&options(public, false, Some("t"))),
                Err("public HTTP bind requires --unsafe-bind-public".into()),
                "{public:?}"
            );
        }
        assert_eq!(
            validate_http_bind(&options("0.0.0.0", true, None)),
            Err("public HTTP bind requires a bearer token".into())
        );
        assert_eq!(
            validate_http_bind(&options("0.0.0.0", true, Some("t"))),
            Ok("0.0.0.0".into())
        );
        let mut bad_path = options("127.0.0.1", false, None);
        bad_path.path = "mcp".into();
        assert!(validate_http_bind(&bad_path).is_err());
    }
}
