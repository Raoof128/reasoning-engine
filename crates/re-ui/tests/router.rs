//! The audit UI over a real store, driven in-process.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use http_body_util::BodyExt;
use re_model::records::ResearchRun;
use re_store::Store;
use re_ui::{UiOptions, router};
use tower::ServiceExt;

struct Fixture {
    _dir: tempfile::TempDir,
    store: Arc<Store>,
    session: String,
    run: String,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("ui.db")).unwrap());
    let session = store.create_session("Do PRMs generalise?").unwrap().id;
    let root = store
        .register_branch(&session, &["survey".into()], &[], None, 0)
        .unwrap()
        .id;
    store
        .register_branch(&session, &["dig in".into()], &[], Some(&root), 1)
        .unwrap();
    let run = ResearchRun::create("<script>alert(1)</script> PRMs", "general", "standard");
    store.save_run(&run).unwrap();
    store
        .append_provenance(&run.run_id, "run_created", &serde_json::Map::new())
        .unwrap();
    Fixture {
        _dir: dir,
        store,
        session,
        run: run.run_id,
    }
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8(body.to_vec()).unwrap())
}

fn get(path: &str) -> Request<Body> {
    Request::get(path)
        .header(header::HOST, "127.0.0.1:8765")
        .body(Body::empty())
        .unwrap()
}

fn loopback() -> Vec<String> {
    ["127.0.0.1", "localhost", "::1"].map(String::from).to_vec()
}

#[tokio::test]
async fn pages_render_with_strict_headers_and_escaped_text() {
    let f = fixture();
    let app = router(Arc::clone(&f.store), UiOptions::default());

    assert_eq!(send(&app, get("/ui")).await.0, StatusCode::OK);
    let (status, headers, body) = send(&app, get("/ui/")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Do PRMs generalise?"));
    assert!(body.contains(&format!("/ui/sessions/{}", f.session)));
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(csp.contains("default-src 'none'") && !csp.contains("script-src"));
    assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(!body.contains("<script>"), "stored text is escaped");
    assert!(body.contains("&#60;script&#62;"));

    let (status, _, body) = send(&app, get(&format!("/ui/sessions/{}", f.session))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.matches("<circle").count(), 2, "two branch nodes");
    assert_eq!(body.matches("class=\"edge\"").count(), 1, "one parent edge");

    let (status, _, body) = send(&app, get(&format!("/ui/runs/{}", f.run))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("run_created"), "provenance is listed");
    assert!(!body.contains("<script>"));

    let (status, headers, _) = send(&app, get("/ui/ui.css")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/css")
    );
    let (status, headers, _) = send(&app, get("/ui/favicon.svg")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/svg+xml");
}

#[tokio::test]
async fn unknown_ids_and_writes_are_refused() {
    let f = fixture();
    let app = router(Arc::clone(&f.store), UiOptions::default());
    assert_eq!(
        send(&app, get("/ui/elsewhere")).await.0,
        StatusCode::NOT_FOUND
    );
    for path in ["/ui/sessions/nope", "/ui/runs/nope"] {
        let (status, headers, body) = send(&app, get(path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert!(headers.contains_key(header::CONTENT_SECURITY_POLICY));
        assert!(!body.contains("nope"), "the ID is not echoed");
    }
    let post = Request::builder()
        .method(Method::POST)
        .uri("/ui/")
        .header(header::HOST, "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, post).await.0, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn a_token_is_required_as_bearer_or_basic_password() {
    let f = fixture();
    let app = router(
        Arc::clone(&f.store),
        UiOptions {
            token: Some("s3cret".into()),
            allowed_hosts: Some(loopback()),
        },
    );
    let (status, headers, body) = send(&app, get("/ui/")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        headers[header::WWW_AUTHENTICATE]
            .to_str()
            .unwrap()
            .starts_with("Basic ")
    );
    assert!(!body.contains("Do PRMs"));

    let with = |value: String| {
        Request::get("/ui/")
            .header(header::HOST, "localhost:8765")
            .header(header::AUTHORIZATION, value)
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        send(&app, with("Bearer s3cret".into())).await.0,
        StatusCode::OK
    );
    let basic = |creds: &str| format!("Basic {}", STANDARD.encode(creds));
    assert_eq!(
        send(&app, with(basic("anyone:s3cret"))).await.0,
        StatusCode::OK
    );
    for bad in [
        "Bearer wrong".to_string(),
        "Bearer s3cret2".to_string(),
        basic("s3cret:wrong"),
        basic("s3cret"),
        "Basic !!!".to_string(),
        "s3cret".to_string(),
    ] {
        assert_eq!(
            send(&app, with(bad.clone())).await.0,
            StatusCode::UNAUTHORIZED,
            "{bad}"
        );
    }
}

#[tokio::test]
async fn a_loopback_bind_refuses_foreign_hosts() {
    let f = fixture();
    let app = router(
        Arc::clone(&f.store),
        UiOptions {
            token: None,
            allowed_hosts: Some(loopback()),
        },
    );
    let at = |host: &str| {
        Request::get("/ui/")
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap()
    };
    for host in ["127.0.0.1:8765", "localhost", "LOCALHOST:1", "[::1]:8765"] {
        assert_eq!(send(&app, at(host)).await.0, StatusCode::OK, "{host}");
    }
    for host in ["evil.example:8765", "127.0.0.1.evil.example", "[::2]:8765"] {
        assert_eq!(
            send(&app, at(host)).await.0,
            StatusCode::FORBIDDEN,
            "{host}"
        );
    }
    let no_host = Request::get("/ui/").body(Body::empty()).unwrap();
    assert_eq!(send(&app, no_host).await.0, StatusCode::FORBIDDEN);
}
