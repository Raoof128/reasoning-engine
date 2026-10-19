//! `reasoning-engine setup`: the only subcommand that ever prompts.
//!
//! It refuses to prompt when stdin is not a terminal (the MCP protocol owns
//! stdin there), takes every answer from a flag when given, and never runs
//! anything external unless the user explicitly says yes.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use inquire::{Confirm, CustomType, Text};

use crate::config::{Config, config_path, expand_user};

/// Exit code when `setup` cannot prompt.
pub const EXIT_NO_TTY: u8 = 2;

/// Answers given as flags; `None` means ask (or keep the current value).
#[derive(Debug, Clone, Default, clap::Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one field per command-line flag"
)]
pub struct SetupArgs {
    /// Accept current values or defaults for everything not given as a flag.
    #[arg(long)]
    pub yes: bool,
    /// Delete config.toml first. The database and run packs are kept.
    #[arg(long)]
    pub reset: bool,
    /// Where the database, run packs and secrets live.
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Verifier tier.
    #[arg(long)]
    pub verifier: Option<String>,
    /// Enable the local HTTP MCP server (`--no-http` disables it).
    #[arg(long, overrides_with = "no_http")]
    pub http: bool,
    /// Disable the local HTTP MCP server.
    #[arg(long)]
    pub no_http: bool,
    /// Local HTTP port.
    #[arg(long)]
    pub port: Option<u16>,
    /// Enable the local audit UI (`--no-ui` disables it).
    #[arg(long, overrides_with = "no_ui")]
    pub ui: bool,
    /// Disable the local audit UI.
    #[arg(long)]
    pub no_ui: bool,
    /// Enable the Cloudflare tunnel (`--no-tunnel` disables it).
    #[arg(long, overrides_with = "no_tunnel")]
    pub tunnel: bool,
    /// Disable the Cloudflare tunnel.
    #[arg(long)]
    pub no_tunnel: bool,
    /// Run `claude mcp add` for this binary (`--no-register` skips it).
    #[arg(long, overrides_with = "no_register")]
    pub register: bool,
    /// Do not register with Claude Code.
    #[arg(long)]
    pub no_register: bool,
}

fn flag(on: bool, off: bool) -> Option<bool> {
    match (on, off) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

/// How answers are obtained for one run.
struct Asker {
    interactive: bool,
}

impl Asker {
    fn confirm(&self, given: Option<bool>, question: &str, default: bool) -> Result<bool, String> {
        match given {
            Some(answer) => Ok(answer),
            None if self.interactive => Confirm::new(question)
                .with_default(default)
                .prompt()
                .map_err(|e| e.to_string()),
            None => Ok(default),
        }
    }
}

/// A 256-bit bearer token from the OS RNG (via two v4 UUIDs' random bits
/// plus a third for margin), as lowercase hex.
fn new_token() -> String {
    (0..3)
        .map(|_| uuid::Uuid::new_v4().simple().to_string())
        .collect()
}

fn write_secret(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    std::io::Write::write_all(&mut options.open(path)?, contents.as_bytes())
}

fn on_path(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn register_with_claude() -> String {
    let exe = std::env::current_exe()
        .map_or_else(|_| "reasoning-engine".into(), |p| p.display().to_string());
    let args = [
        "mcp",
        "add",
        "reasoning-engine",
        "-s",
        "user",
        "--",
        exe.as_str(),
        "mcp",
    ];
    let manual = format!("claude {}", args.join(" "));
    match Command::new("claude").args(args).status() {
        Ok(status) if status.success() => "registered with Claude Code".into(),
        Ok(status) => format!("`claude mcp add` failed ({status}); run it yourself: {manual}"),
        Err(_) => format!("`claude` is not on PATH; run this once it is: {manual}"),
    }
}

/// Validate and apply `--verifier`, then say which tier is in effect.
fn choose_verifier(
    config: &mut Config,
    requested: Option<&str>,
    interactive: bool,
) -> Result<(), String> {
    if let Some(verifier) = requested {
        if !re_verify::verify::TIERS.contains(&verifier) {
            return Err(format!(
                "unknown verifier: {verifier} (expected one of: {})",
                re_verify::verify::TIERS.join(", ")
            ));
        }
        config.verifier = verifier.to_string();
    }
    if config.verifier != "lexical" && !cfg!(feature = "semantic") {
        eprintln!(
            "note: verifier {} needs a build with --features semantic",
            config.verifier
        );
    }
    if interactive {
        eprintln!(
            "Verifier tier: {} (lexical, or hybrid/nli with --features semantic)",
            config.verifier
        );
    }
    Ok(())
}

/// Run setup. Returns the process exit code.
///
/// # Errors
/// A prompt was cancelled, or the config could not be written.
pub fn run(args: &SetupArgs) -> Result<u8, String> {
    let path = config_path();
    if args.reset {
        match std::fs::remove_file(&path) {
            Ok(()) => eprintln!("removed {} (database and run packs kept)", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("could not remove {}: {e}", path.display())),
        }
    }
    let interactive = !args.yes && std::io::stdin().is_terminal();
    if !args.yes && !interactive {
        eprintln!(
            "setup needs an interactive terminal; rerun with --yes to accept defaults, \
             or pass answers as flags (see `reasoning-engine setup --help`)"
        );
        return Ok(EXIT_NO_TTY);
    }
    let ask = Asker { interactive };
    let mut config = Config::load(&path)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();

    config.data_dir = match (&args.data_dir, interactive) {
        (Some(dir), _) => dir.clone(),
        (None, true) => Text::new("Data directory")
            .with_default(&config.data_dir.display().to_string())
            .prompt()
            .map(PathBuf::from)
            .map_err(|e| e.to_string())?,
        (None, false) => config.data_dir.clone(),
    };
    choose_verifier(&mut config, args.verifier.as_deref(), interactive)?;

    config.http.enabled = ask.confirm(
        flag(args.http, args.no_http),
        "Set up a local HTTP MCP server?",
        config.http.enabled,
    )?;
    if config.http.enabled {
        config.http.port = match (args.port, interactive) {
            (Some(port), _) => port,
            (None, true) => CustomType::<u16>::new("Port")
                .with_default(config.http.port)
                .prompt()
                .map_err(|e| e.to_string())?,
            (None, false) => config.http.port,
        };
        let has_token = config
            .http
            .token_file
            .as_deref()
            .is_some_and(|p| expand_user(p).exists());
        if ask.confirm(None, "Generate a bearer token?", !has_token)? {
            let token_file = config.data_dir.join("http.env");
            write_secret(
                &expand_user(&token_file),
                &format!("REASONING_ENGINE_HTTP_TOKEN={}\n", new_token()),
            )
            .map_err(|e| format!("could not write token file: {e}"))?;
            eprintln!(
                "bearer token written to {} (mode 0600)",
                token_file.display()
            );
            config.http.token_file = Some(token_file);
        }
    }
    config.ui.enabled = ask.confirm(
        flag(args.ui, args.no_ui),
        "Enable the local audit UI?",
        config.ui.enabled,
    )?;
    config.tunnel.enabled = ask.confirm(
        flag(args.tunnel, args.no_tunnel),
        "Set up the Notion/Cloudflare tunnel?",
        config.tunnel.enabled,
    )?;
    if config.tunnel.enabled && !on_path("cloudflared") {
        eprintln!("note: the tunnel needs `cloudflared` on PATH");
    }

    std::fs::create_dir_all(config.data_dir())
        .map_err(|e| format!("could not create data directory: {e}"))?;
    config.save(&path).map_err(|e| e.to_string())?;
    eprintln!("saved {}", path.display());

    // Registering runs an external command, so it needs an explicit yes.
    if ask.confirm(
        flag(args.register, args.no_register),
        "Register with Claude Code now?",
        false,
    )? {
        eprintln!("{}", register_with_claude());
    }
    Ok(0)
}
