#!/usr/bin/env bash
set -euo pipefail

APP_NAME="reasoning-engine"
PORT="${PORT:-8765}"
HOST="127.0.0.1"
CONFIG_DIR="$HOME/.reasoning-engine"
ENV_FILE="$CONFIG_DIR/notion-http.env"
LOG_DIR="$CONFIG_DIR/logs"
SERVER_LOG="$LOG_DIR/mcp-server.log"
TUNNEL_LOG="$LOG_DIR/cloudflared.log"

mkdir -p "$CONFIG_DIR" "$LOG_DIR"

bold() { printf "\033[1m%s\033[0m\n" "$*"; }
info() { printf "[info] %s\n" "$*"; }
warn() { printf "[warn] %s\n" "$*"; }
fail() {
  printf "[error] %s\n" "$*" >&2
  exit 1
}

cleanup() {
  warn "Stopping local MCP server and Cloudflare tunnel..."
  if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
  fi
  if [[ -n "${TUNNEL_PID:-}" ]] && kill -0 "$TUNNEL_PID" 2>/dev/null; then
    kill "$TUNNEL_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

bold "Verifiable Research Engine to Notion laptop HTTPS launcher"

if [[ "$(uname -s)" != "Darwin" ]]; then
  fail "This script is written for macOS. Your system is $(uname -s)."
fi

if ! command -v brew >/dev/null 2>&1; then
  fail "Homebrew is not installed. Install it from https://brew.sh, then rerun this script."
fi

if ! command -v cloudflared >/dev/null 2>&1; then
  warn "cloudflared not found. Installing with Homebrew..."
  brew install cloudflared
fi

if ! command -v "$APP_NAME" >/dev/null 2>&1; then
  warn "$APP_NAME command not found."

  if [[ -f "pyproject.toml" ]]; then
    warn "pyproject.toml found. Installing this project in editable mode..."
    python3 -m venv .venv
    # shellcheck disable=SC1091
    source .venv/bin/activate
    python -m pip install --upgrade pip
    python -m pip install -e .
  else
    fail "$APP_NAME is not installed and the current directory is not the project root."
  fi
fi

if [[ ! -f "$ENV_FILE" ]]; then
  TOKEN="$(openssl rand -hex 32)"
  cat >"$ENV_FILE" <<EOF
REASONING_ENGINE_HTTP_TOKEN=$TOKEN
EOF
  chmod 600 "$ENV_FILE"
  info "Created bearer token file at $ENV_FILE"
else
  chmod 600 "$ENV_FILE"
  info "Using existing bearer token file at $ENV_FILE"
fi

# shellcheck disable=SC1090
source "$ENV_FILE"

if [[ -z "${REASONING_ENGINE_HTTP_TOKEN:-}" ]]; then
  fail "REASONING_ENGINE_HTTP_TOKEN is missing from $ENV_FILE"
fi
export REASONING_ENGINE_HTTP_TOKEN

bold "Starting local MCP server on http://$HOST:$PORT/mcp"

"$APP_NAME" serve \
  --transport http \
  --host "$HOST" \
  --port "$PORT" \
  --bearer-token-env REASONING_ENGINE_HTTP_TOKEN \
  >"$SERVER_LOG" 2>&1 &

SERVER_PID="$!"

sleep 2

if ! kill -0 "$SERVER_PID" 2>/dev/null; then
  cat "$SERVER_LOG" >&2 || true
  fail "MCP server failed to start."
fi

info "MCP server running. Logs: $SERVER_LOG"

bold "Starting Cloudflare temporary HTTPS tunnel..."

cloudflared tunnel --url "http://$HOST:$PORT" >"$TUNNEL_LOG" 2>&1 &
TUNNEL_PID="$!"

TUNNEL_URL=""

for _ in {1..30}; do
  if grep -Eo 'https://[-a-zA-Z0-9.]+\.trycloudflare\.com' "$TUNNEL_LOG" >/dev/null 2>&1; then
    TUNNEL_URL="$(grep -Eo 'https://[-a-zA-Z0-9.]+\.trycloudflare\.com' "$TUNNEL_LOG" | head -n 1)"
    break
  fi
  sleep 1
done

if [[ -z "$TUNNEL_URL" ]]; then
  cat "$TUNNEL_LOG" >&2 || true
  fail "Could not detect Cloudflare tunnel URL."
fi

NOTION_MCP_URL="$TUNNEL_URL/mcp"

bold "Notion MCP connection details"
echo
echo "Server URL:"
echo "$NOTION_MCP_URL"
echo
echo "Header:"
echo "Authorization: Bearer <token from $ENV_FILE>"
echo
echo "Token file:"
echo "$ENV_FILE"
echo
warn "Do not paste the token into GitHub, logs, screenshots, or public chats."
warn "Temporary Cloudflare URLs change each run. Update Notion when this script restarts."
warn "Keep this terminal open. Press Ctrl+C to stop the server and tunnel."
echo
bold "Logs"
echo "MCP server: $SERVER_LOG"
echo "Tunnel:     $TUNNEL_LOG"
echo

while true; do
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    cat "$SERVER_LOG" >&2 || true
    fail "MCP server stopped unexpectedly."
  fi

  if ! kill -0 "$TUNNEL_PID" 2>/dev/null; then
    cat "$TUNNEL_LOG" >&2 || true
    fail "Cloudflare tunnel stopped unexpectedly."
  fi

  sleep 5
done
