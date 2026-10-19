//! `reasoning-engine serve` (`local-host` feature): the opt-in local
//! Streamable HTTP server. It refuses to start unless `setup` enabled it or
//! `--http` is given. The read-only audit UI is mounted at `/ui` only when
//! `setup` enabled it or `--ui` is given.

use std::path::Path;
use std::sync::Arc;

use re_mcp::ReasoningServer;
use re_mcp::http::{HttpOptions, LOOPBACK_HOSTS, is_loopback, serve_with};
use re_store::Store;
use re_ui::UiOptions;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::config::{Config, expand_user};

/// Exit code when HTTP is not enabled.
pub const EXIT_NOT_ENABLED: u8 = 2;
const TOKEN_KEY: &str = "REASONING_ENGINE_HTTP_TOKEN=";

/// `serve` flags; unset ones come from `config.toml`.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct ServeArgs {
    /// Serve even if `setup` left the HTTP server disabled.
    #[arg(long)]
    pub http: bool,
    /// The Python CLI's spelling of `--http`, accepted so existing scripts
    /// and launchers keep working.
    #[arg(long, value_parser = ["http"], hide = true)]
    pub transport: Option<String>,
    /// Bind address (loopback unless --unsafe-bind-public).
    #[arg(long)]
    pub host: Option<String>,
    /// Port.
    #[arg(long)]
    pub port: Option<u16>,
    /// MCP endpoint path.
    #[arg(long)]
    pub path: Option<String>,
    /// Allow a non-loopback bind (also requires a bearer token).
    #[arg(long)]
    pub unsafe_bind_public: bool,
    /// Read the bearer token from this environment variable instead of the
    /// configured token file.
    #[arg(long)]
    pub bearer_token_env: Option<String>,
    /// Mount the read-only audit UI at /ui even if `setup` left it off.
    #[arg(long)]
    pub ui: bool,
}

/// The audit UI's access rules: the server's token, and loopback `Host`
/// names only on a loopback bind.
#[must_use]
pub fn ui_options(options: &HttpOptions) -> UiOptions {
    UiOptions {
        token: options.bearer_token.clone(),
        allowed_hosts: is_loopback(re_core::pystr::py_strip(&options.host))
            .then(|| LOOPBACK_HOSTS.map(String::from).to_vec()),
    }
}

/// The token from a `KEY=value` token file written by `setup`.
fn read_token_file(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read token file {}: {e}", path.display()))?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix(TOKEN_KEY))
        .map(str::to_string)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| format!("no {TOKEN_KEY}... line in {}", path.display()))
}

fn bearer_token(config: &Config, args: &ServeArgs) -> Result<Option<String>, String> {
    if let Some(var) = &args.bearer_token_env {
        let token = std::env::var(var).ok().filter(|t| !t.is_empty());
        let token = token.ok_or_else(|| format!("{var} is not set or is empty"))?;
        // Same confirmation line as Python, never the value itself.
        let line =
            json!({"bearer_token_env": var, "has_bearer_token": true, "token_value": "<redacted>"});
        println!(
            "{}",
            re_core::pyjson::dumps(&line, &re_core::pyjson::DumpOptions::sorted())
        );
        return Ok(Some(token));
    }
    match &config.http.token_file {
        Some(path) => read_token_file(&expand_user(path)).map(Some),
        None => Ok(None),
    }
}

fn axum_router() -> re_mcp::http::Router {
    re_mcp::http::Router::new()
}

/// Run the HTTP server until Ctrl-C. Returns the process exit code.
///
/// # Errors
/// A missing token, an invalid bind, or a socket failure.
pub async fn run(
    config: &Config,
    server: ReasoningServer,
    args: &ServeArgs,
) -> Result<u8, Box<dyn std::error::Error + Send + Sync>> {
    if !config.http.enabled && !args.http && args.transport.is_none() {
        eprintln!(
            "the local HTTP server is off; enable it with `reasoning-engine setup --http` \
             or pass --http"
        );
        return Ok(EXIT_NOT_ENABLED);
    }
    let options = HttpOptions {
        host: args
            .host
            .clone()
            .unwrap_or_else(|| config.http.host.clone()),
        port: args.port.unwrap_or(config.http.port),
        path: args
            .path
            .clone()
            .unwrap_or_else(|| config.http.path.clone()),
        bearer_token: bearer_token(config, args)?,
        unsafe_bind_public: args.unsafe_bind_public,
    };
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        stop.cancel();
    });
    let guarded = if options.bearer_token.is_some() {
        "bearer token required"
    } else {
        "no token"
    };
    let ui = config.ui.enabled || args.ui;
    let extra = if ui {
        let store = Arc::new(Store::open(config.db_path())?);
        re_ui::router(store, ui_options(&options))
    } else {
        axum_router()
    };
    let path = options.path.clone();
    serve_with(server, &options, extra, shutdown, |addr| {
        eprintln!("serving MCP at http://{addr}{path} ({guarded}); Ctrl-C to stop");
        if ui {
            eprintln!("audit UI at http://{addr}{}/", re_ui::PREFIX);
        }
    })
    .await?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(host: &str, token: Option<&str>) -> HttpOptions {
        HttpOptions {
            host: host.into(),
            port: 8765,
            path: "/mcp".into(),
            bearer_token: token.map(Into::into),
            unsafe_bind_public: true,
        }
    }

    #[test]
    fn the_ui_shares_the_token_and_pins_loopback_hosts() {
        let local = ui_options(&options(" 127.0.0.1 ", None));
        assert_eq!(local.token, None);
        assert!(
            local
                .allowed_hosts
                .is_some_and(|hosts| hosts.contains(&"localhost".to_string()))
        );
        let public = ui_options(&options("0.0.0.0", Some("t")));
        assert_eq!(public.token.as_deref(), Some("t"));
        assert_eq!(public.allowed_hosts, None, "the token guards a public bind");
    }
}
