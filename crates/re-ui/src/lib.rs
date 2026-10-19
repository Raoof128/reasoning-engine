//! Read-only local audit UI (plan Phase 6), mounted at `/ui` by
//! `reasoning-engine serve` only when `setup` enabled it (`[ui] enabled`) or
//! `--ui` is given.
//!
//! Pages: recent sessions and research runs; per session the branch tree
//! (inline SVG), scores per branch, branches and reflections; per run the
//! evidence table with risk flags, claims with their latest verification,
//! gaps and provenance.
//!
//! Hardening, on top of the server's loopback-only default:
//!
//! - `GET`/`HEAD` only; nothing writes;
//! - with a token, every request needs `Authorization: Bearer <token>` or
//!   HTTP Basic with the token as the password (browsers cannot send a
//!   bearer header when opening a page; Basic makes them prompt), compared
//!   in constant time;
//! - on a loopback bind, only loopback `Host` headers (DNS rebinding);
//! - a strict CSP with no script source at all, `nosniff`, no framing, no
//!   referrer, no caching; stored URLs become links only when they are
//!   absolute `http(s)`, and askama escapes all text.

use std::sync::Arc;

use askama::Template;
use axum::Router;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use re_store::{Store, StoreError};
use subtle::ConstantTimeEq;

pub mod view;

use view::{BranchItem, ClaimItem, EventItem, EvidenceItem, ReflectionItem, RunItem, SessionItem};

/// Where the UI is mounted.
pub const PREFIX: &str = "/ui";
/// Rows on the index page per list.
pub const INDEX_LIMIT: usize = 50;
const CSS: &str = include_str!("../templates/ui.css");
const FAVICON: &str = include_str!("../templates/favicon.svg");
const CSP: &str = "default-src 'none'; style-src 'self'; img-src 'self'; base-uri 'none'; \
                   form-action 'none'; frame-ancestors 'none'";

/// Access rules for the UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiOptions {
    /// Required secret (bearer, or the Basic password), if any.
    pub token: Option<String>,
    /// Accepted `Host` names (port ignored); `None` accepts any host.
    pub allowed_hosts: Option<Vec<String>>,
}

#[derive(Clone)]
struct AppState {
    store: Arc<Store>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage {
    sessions: Vec<SessionItem>,
    runs: Vec<RunItem>,
    limit: usize,
}

#[derive(Template)]
#[template(path = "session.html")]
struct SessionPage {
    session: SessionItem,
    tree: view::Tree,
    chart: view::Chart,
    branches: Vec<BranchItem>,
    reflections: Vec<ReflectionItem>,
}

#[derive(Template)]
#[template(path = "run.html")]
struct RunPage {
    run: RunItem,
    evidence: Vec<EvidenceItem>,
    claims: Vec<ClaimItem>,
    gaps: Vec<EventItem>,
    provenance: Vec<EventItem>,
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorPage {
    status: u16,
    message: String,
}

fn error_page(status: StatusCode, message: &str) -> Response {
    let page = ErrorPage {
        status: status.as_u16(),
        message: message.to_string(),
    };
    match page.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => status.into_response(),
    }
}

fn render(page: &impl Template) -> Response {
    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => error_page(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the page failed to render",
        ),
    }
}

/// Run a store query off the async runtime. `Invalid` (unknown ID) is a
/// 404; anything else is a 500 whose detail stays out of the page.
async fn query<T: Send + 'static>(
    state: &AppState,
    f: impl FnOnce(&Store) -> Result<T, StoreError> + Send + 'static,
) -> Result<T, Box<Response>> {
    let store = Arc::clone(&state.store);
    let page = match tokio::task::spawn_blocking(move || f(&store)).await {
        Ok(Ok(value)) => return Ok(value),
        Ok(Err(StoreError::Invalid(_))) => error_page(StatusCode::NOT_FOUND, "not found"),
        Ok(Err(error)) => {
            eprintln!("audit ui: store error: {error}");
            error_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the database could not be read",
            )
        }
        Err(_) => error_page(StatusCode::INTERNAL_SERVER_ERROR, "the query failed"),
    };
    Err(Box::new(page))
}

async fn index(State(state): State<AppState>) -> Response {
    let lists = query(&state, |store| {
        Ok((
            store.list_sessions(INDEX_LIMIT)?,
            store.list_runs(INDEX_LIMIT)?,
        ))
    })
    .await;
    match lists {
        Ok((sessions, runs)) => render(&IndexPage {
            sessions: sessions.iter().map(SessionItem::from).collect(),
            runs: runs.iter().map(RunItem::from).collect(),
            limit: INDEX_LIMIT,
        }),
        Err(response) => *response,
    }
}

async fn session(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let data = query(&state, move |store| {
        Ok((
            store.get_session(&id)?,
            store.get_session_branches(&id)?,
            store.list_reflections(&id)?,
        ))
    })
    .await;
    match data {
        Ok((session, branches, reflections)) => render(&SessionPage {
            session: SessionItem::from(&session),
            tree: view::tree(&branches),
            chart: view::chart(&branches),
            branches: branches.iter().map(BranchItem::from).collect(),
            reflections: reflections.iter().map(ReflectionItem::from).collect(),
        }),
        Err(response) => *response,
    }
}

async fn run(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let data = query(&state, move |store| {
        Ok((
            store.get_run(&id)?,
            store.list_evidence(&id)?,
            store.list_claims(&id)?,
            store.list_verifications(&id)?,
            store.list_gaps(&id)?,
            store.list_provenance(&id)?,
        ))
    })
    .await;
    match data {
        Ok((run, evidence, claims, verifications, gaps, provenance)) => render(&RunPage {
            run: RunItem::from(&run),
            evidence: view::evidence(&evidence),
            claims: view::claims(&claims, &verifications),
            gaps: view::gaps(&gaps),
            provenance: view::provenance(&provenance),
        }),
        Err(response) => *response,
    }
}

async fn favicon() -> Response {
    ([(header::CONTENT_TYPE, "image/svg+xml")], FAVICON).into_response()
}

async fn stylesheet() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], CSS).into_response()
}

/// The secret a request presents: a bearer token, or a Basic password.
fn presented(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    if let Some(token) = value.strip_prefix("Bearer ") {
        return Some(token.to_string());
    }
    let encoded = value.strip_prefix("Basic ")?;
    let decoded = String::from_utf8(STANDARD.decode(encoded.trim()).ok()?).ok()?;
    decoded
        .split_once(':')
        .map(|(_, password)| password.to_string())
}

fn host_allowed(headers: &HeaderMap, allowed: &[String]) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    // `[::1]:8765`, `localhost:8765`, `127.0.0.1`.
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    allowed.iter().any(|a| a.eq_ignore_ascii_case(name))
}

async fn guard(State(options): State<Arc<UiOptions>>, request: Request, next: Next) -> Response {
    if let Some(allowed) = &options.allowed_hosts
        && !host_allowed(request.headers(), allowed)
    {
        return error_page(StatusCode::FORBIDDEN, "host not allowed");
    }
    if let Some(token) = options.token.as_deref().filter(|t| !t.is_empty()) {
        let ok = presented(request.headers())
            .is_some_and(|p| bool::from(p.as_bytes().ct_eq(token.as_bytes())));
        if !ok {
            let mut response =
                error_page(StatusCode::UNAUTHORIZED, "sign in with the server token");
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static(
                    "Basic realm=\"reasoning-engine audit\", charset=\"UTF-8\"",
                ),
            );
            return response;
        }
    }
    next.run(request).await
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        (header::CONTENT_SECURITY_POLICY, CSP),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::CACHE_CONTROL, "no-store"),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

/// The UI router, with every route under [`PREFIX`] and no fallback, so
/// it merges into the server's router without capturing other paths.
pub fn router(store: Arc<Store>, options: UiOptions) -> Router {
    let p = |path: &str| format!("{PREFIX}{path}");
    Router::new()
        .route(PREFIX, get(index))
        .route(&p("/"), get(index))
        .route(&p("/sessions/{id}"), get(session))
        .route(&p("/runs/{id}"), get(run))
        .route(&p("/ui.css"), get(stylesheet))
        .route(&p("/favicon.svg"), get(favicon))
        .with_state(AppState { store })
        .route_layer(middleware::from_fn_with_state(Arc::new(options), guard))
        .route_layer(middleware::from_fn(security_headers))
}
