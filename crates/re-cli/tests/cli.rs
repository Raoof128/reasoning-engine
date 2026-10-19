//! The `reasoning-engine` binary, end to end, in an isolated home.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};

use assert_cmd::Command;
use serde_json::Value;

/// Variables that would point the binary outside the test's home. The
/// rest of the environment stays (Windows needs `SystemRoot` and friends).
const ENGINE_VARS: [&str; 8] = [
    "REASONING_ENGINE_SIGNING_KEY",
    "REASONING_ENGINE_DB",
    "REASONING_ENGINE_RUNS_DIR",
    "REASONING_ENGINE_VERIFIER",
    "REASONING_ENGINE_SOURCES",
    "REASONING_ENGINE_CONTACT_EMAIL",
    "SCHOLAR_GATEWAY_LIVE",
    "SCHOLAR_GATEWAY_ACCESS_TOKEN",
];

/// The binary with every path pointed into `home` and the network off.
fn bin(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("reasoning-engine").expect("binary builds");
    for var in ENGINE_VARS {
        cmd.env_remove(var);
    }
    cmd.env("HOME", home)
        .env("USERPROFILE", home)
        .env("REASONING_ENGINE_CONFIG", home.join("config.toml"))
        .env("REASONING_ENGINE_OFFLINE", "1");
    cmd
}

fn stdout_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout is JSON")
}

#[test]
fn arguments_are_validated() {
    let home = tempfile::tempdir().expect("tmp");
    bin(home.path()).args(["research", "q"]).assert().code(2);
    bin(home.path()).arg("nope").assert().code(2);
    bin(home.path())
        .args(["scholar", "search", "q", "--limit", "x"])
        .assert()
        .code(2);
}

#[test]
fn setup_refuses_to_prompt_without_a_terminal() {
    let home = tempfile::tempdir().expect("tmp");
    let output = bin(home.path()).arg("setup").output().expect("runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--yes"));
    assert!(!home.path().join("config.toml").exists());
}

#[test]
fn setup_yes_writes_config_and_a_private_token() {
    let home = tempfile::tempdir().expect("tmp");
    let data = home.path().join("data");
    bin(home.path())
        .args([
            "setup",
            "--yes",
            "--http",
            "--port",
            "9100",
            "--no-register",
            "--data-dir",
        ])
        .arg(&data)
        .assert()
        .success();
    let config = std::fs::read_to_string(home.path().join("config.toml")).expect("config");
    assert!(config.contains("port = 9100"), "{config}");
    assert!(config.contains("enabled = true"), "{config}");
    let token = std::fs::read_to_string(data.join("http.env")).expect("token file");
    assert!(
        token.starts_with("REASONING_ENGINE_HTTP_TOKEN=") && token.trim_end().len() > 90,
        "{token}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(data.join("http.env"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    bin(home.path())
        .args(["setup", "--reset", "--yes", "--no-register"])
        .assert()
        .success();
    let reset = std::fs::read_to_string(home.path().join("config.toml")).expect("config");
    assert!(
        reset.contains("enabled = false"),
        "reset restores defaults: {reset}"
    );
    bin(home.path())
        .args(["setup", "--yes", "--verifier", "oracle"])
        .assert()
        .code(1);
}

#[test]
fn research_prints_a_pack_that_verifies_and_hints_once() {
    let home = tempfile::tempdir().expect("tmp");
    let output = bin(home.path())
        .args([
            "research",
            "prompt injection",
            "--draft",
            "Mock evidence for prompt injection.",
        ])
        .output()
        .expect("runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("run `reasoning-engine setup`").count(),
        1,
        "{stderr}"
    );
    let payload = stdout_json(&output);
    assert_eq!(payload["attestation"]["valid"], true);
    assert_eq!(payload["quality_gate"]["result"], "pass");
    let text = String::from_utf8(output.stdout.clone()).expect("utf-8");
    assert!(
        text.starts_with("{\n  \"attestation\": {"),
        "sorted keys, indent 2"
    );

    let pack = payload["run_pack"].as_str().expect("path");
    assert!(Path::new(pack).starts_with(home.path().join(".reasoning-engine").join("runs")));
    let verified = bin(home.path())
        .args(["verify-runpack", pack])
        .output()
        .expect("runs");
    assert!(verified.status.success());
    assert_eq!(stdout_json(&verified)["valid"], true);

    std::fs::write(Path::new(pack).join("report.md"), "tampered").expect("tamper");
    bin(home.path())
        .args(["verify-runpack", pack])
        .assert()
        .code(1);
}

#[test]
fn scholar_search_omits_records_like_python() {
    let home = tempfile::tempdir().expect("tmp");
    let output = bin(home.path())
        .args(["scholar", "search", "reward models", "--limit", "3"])
        .output()
        .expect("runs");
    assert!(output.status.success());
    let payload = stdout_json(&output);
    assert_eq!(payload["evidence"].as_array().expect("evidence").len(), 3);
    let keys: Vec<&String> = payload.as_object().expect("object").keys().collect();
    assert_eq!(keys, ["error", "evidence", "gaps"]);
}

#[test]
fn mcp_answers_initialize_on_stdio() {
    let home = tempfile::tempdir().expect("tmp");
    let path = assert_cmd::cargo::cargo_bin("reasoning-engine");
    let mut command = StdCommand::new(path);
    for var in ENGINE_VARS {
        command.env_remove(var);
    }
    // An empty value means unset, as in Python.
    command.env("REASONING_ENGINE_DB", "");
    let mut child = command
        .arg("mcp")
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("REASONING_ENGINE_CONFIG", home.path().join("config.toml"))
        .env("REASONING_ENGINE_OFFLINE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let mut stdin = child.stdin.take().expect("stdin");
    let request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;
    writeln!(stdin, "{request}").expect("write");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("read");
    drop(stdin);
    let _ = child.wait();
    let response: Value = serde_json::from_str(&line).expect("JSON-RPC response");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["serverInfo"]["name"], "reasoning-engine");
    assert_eq!(
        response["result"]["instructions"],
        "Actor-Critic-Planner-Reflexion reasoning backend for deep research"
    );
}

#[cfg(not(feature = "local-host"))]
#[test]
fn default_builds_have_no_http_server() {
    let home = tempfile::tempdir().expect("tmp");
    bin(home.path()).args(["serve", "--http"]).assert().code(2);
}

#[cfg(feature = "local-host")]
mod serve {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::process::{Command as StdCommand, Stdio};

    use super::{ENGINE_VARS, bin};

    #[test]
    fn serve_refuses_until_enabled() {
        let home = tempfile::tempdir().expect("tmp");
        let output = bin(home.path()).arg("serve").output().expect("runs");
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("setup --http"));
        let missing = bin(home.path())
            .args(["serve", "--http", "--bearer-token-env", "NO_SUCH_TOKEN_VAR"])
            .output()
            .expect("runs");
        assert_eq!(missing.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&missing.stderr)
                .contains("NO_SUCH_TOKEN_VAR is not set or is empty")
        );
        // The Python CLI's `--transport http` opts in like `--http`.
        let python_style = bin(home.path())
            .args(["serve", "--transport", "http"])
            .args(["--bearer-token-env", "NO_SUCH_TOKEN_VAR"])
            .output()
            .expect("runs");
        assert_eq!(python_style.status.code(), Some(1));
        let stdio = bin(home.path())
            .args(["serve", "--transport", "stdio"])
            .output()
            .expect("runs");
        assert_eq!(stdio.status.code(), Some(2), "only http, as in Python");
    }

    fn post(port: u16, token: Option<&str>) -> String {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream.write_all(request.as_bytes()).expect("write");
        let mut status = String::new();
        BufReader::new(&mut stream)
            .read_line(&mut status)
            .expect("status line");
        let mut rest = String::new();
        let _ = stream.read_to_string(&mut rest);
        status
    }

    #[test]
    fn serve_uses_the_token_from_setup() {
        let home = tempfile::tempdir().expect("tmp");
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port();
        bin(home.path())
            .args([
                "setup",
                "--yes",
                "--http",
                "--no-register",
                "--port",
                &port.to_string(),
            ])
            .assert()
            .success();
        let token_file =
            std::fs::read_to_string(home.path().join(".reasoning-engine/http.env")).expect("token");
        let token = token_file
            .trim()
            .strip_prefix("REASONING_ENGINE_HTTP_TOKEN=")
            .expect("key")
            .to_string();

        let mut command = StdCommand::new(assert_cmd::cargo::cargo_bin("reasoning-engine"));
        for var in ENGINE_VARS {
            command.env_remove(var);
        }
        let mut child = command
            .arg("serve")
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("REASONING_ENGINE_CONFIG", home.path().join("config.toml"))
            .env("REASONING_ENGINE_OFFLINE", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        // Skip any hints until the server reports its address.
        let mut stderr = BufReader::new(child.stderr.take().expect("stderr"));
        let mut banner = String::new();
        while !banner.contains("serving MCP") {
            banner.clear();
            assert_ne!(
                stderr.read_line(&mut banner).expect("read"),
                0,
                "server exited before serving"
            );
        }
        assert!(banner.contains("bearer token required"), "{banner}");
        assert!(!banner.contains(&token), "the token is never printed");

        let unauthorised = post(port, None);
        let authorised = post(port, Some(&token));
        let _ = child.kill();
        let _ = child.wait();
        assert!(unauthorised.contains(" 401 "), "{unauthorised}");
        assert!(authorised.contains(" 200 "), "{authorised}");
    }
}

#[cfg(feature = "trust")]
mod trust {
    use std::path::{Path, PathBuf};

    use super::{bin, stdout_json};

    fn research(home: &Path, record: bool) -> PathBuf {
        let mut args = vec!["research", "Scholar Gateway exposes semantic search"];
        args.extend(["--draft", "Scholar Gateway exposes semantic search."]);
        if record {
            args.push("--record");
        }
        let output = bin(home).args(&args).output().expect("runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        PathBuf::from(stdout_json(&output)["run_pack"].as_str().expect("run_pack"))
    }

    #[test]
    fn packs_are_signed_logged_and_verified_with_exit_codes() {
        let home = tempfile::tempdir().expect("tmp");
        let key = stdout_json(
            &bin(home.path())
                .args(["keys", "show"])
                .output()
                .expect("runs"),
        );
        assert_eq!(key["created"], true);
        let public_key = key["public_key"].as_str().expect("key").to_string();
        let pack = research(home.path(), false);
        let pack = pack.to_str().expect("utf-8");

        let ok = bin(home.path())
            .args(["verify-runpack", pack])
            .output()
            .expect("runs");
        assert_eq!(ok.status.code(), Some(0));
        let report = stdout_json(&ok);
        assert_eq!(report["verdict"], "ok");
        assert_eq!(report["transparency_log"]["included"], true);
        bin(home.path())
            .args(["verify-runpack", pack, "--public-key", &public_key])
            .assert()
            .code(0);
        bin(home.path())
            .args(["verify-runpack", pack, "--public-key", "c29tZW9uZSBlbHNl"])
            .assert()
            .code(1);

        // A pack with no signature verifies its hashes but vouches for
        // nothing: exit 2.
        let attestation = std::path::Path::new(pack).join("attestation.json");
        let text = std::fs::read_to_string(&attestation).expect("read");
        let mut unsigned: serde_json::Value = serde_json::from_str(&text).expect("json");
        for key in ["signature", "signature_algorithm"] {
            unsigned[key] = serde_json::Value::Null;
        }
        std::fs::write(&attestation, unsigned.to_string()).expect("write");
        bin(home.path())
            .args(["verify-runpack", pack])
            .assert()
            .code(2);
    }

    #[test]
    fn recorded_runs_replay_identically_and_unrecorded_ones_are_refused() {
        let home = tempfile::tempdir().expect("tmp");
        let pack = research(home.path(), true);
        assert!(pack.join("replay.json").exists());
        let run_id = pack.file_name().and_then(|n| n.to_str()).expect("run id");
        let replay = bin(home.path())
            .args(["replay", run_id])
            .output()
            .expect("runs");
        assert_eq!(
            replay.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&replay.stdout)
        );
        assert_eq!(stdout_json(&replay)["identical"], true);

        let plain = research(home.path(), false);
        let refused = bin(home.path())
            .args(["replay", plain.to_str().expect("utf-8")])
            .output()
            .expect("runs");
        assert_eq!(refused.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&refused.stderr).contains("was not recorded"));
    }
}

#[cfg(feature = "learn")]
#[test]
fn a_prior_needs_enough_history() {
    let home = tempfile::tempdir().expect("tmp");
    let output = bin(home.path())
        .args(["prior", "train"])
        .output()
        .expect("runs");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("need at least 30 completed sessions"),
        "{stderr}"
    );
    let shown = bin(home.path())
        .args(["prior", "show"])
        .output()
        .expect("runs");
    assert!(shown.status.success());
    assert!(String::from_utf8_lossy(&shown.stdout).starts_with("no prior yet"));
}

#[cfg(feature = "learn")]
#[test]
fn watch_reverifies_a_run_and_reports_drift_by_exit_code() {
    let home = tempfile::tempdir().expect("tmp");
    let research = bin(home.path())
        .args([
            "research",
            "Scholar Gateway exposes semantic search",
            "--draft",
            "Mock evidence for Scholar Gateway exposes semantic search.",
        ])
        .output()
        .expect("runs");
    assert!(research.status.success());
    let run_id = stdout_json(&research)["run_id"]
        .as_str()
        .expect("run id")
        .to_string();

    let watched = bin(home.path())
        .args(["watch", &run_id])
        .output()
        .expect("runs");
    assert_eq!(
        watched.status.code(),
        Some(0),
        "the offline mock never drifts"
    );
    let report = stdout_json(&watched);
    assert_eq!(report["changed"], false);
    assert_ne!(report["new_run_id"], run_id.as_str());
    assert!(
        std::path::Path::new(report["report"].as_str().expect("path")).exists(),
        "{report}"
    );

    bin(home.path())
        .args(["watch", "run_000000000000"])
        .assert()
        .code(1);
    bin(home.path())
        .args(["watch", &run_id, "--every", "soon"])
        .assert()
        .code(2);
}
