#!/usr/bin/env bash
# Capsule Node — one-shot setup.
#
# Usage:
#   ./scripts/setup.sh                              # interactive; prompts for vault path if .env missing
#   ./scripts/setup.sh --vault /path/to/vault       # sets CAPSULE_VAULT_PATH in a new .env
#   ./scripts/setup.sh --non-interactive            # CI mode; requires --vault or existing .env
#   ./scripts/setup.sh --skip-install               # build only; don't copy plugin into vault
#
# What it does:
#   1. Checks prereqs (cargo, rustc, node, npm, jq).
#   2. Creates .env from .env.example if missing.
#   3. cargo build --release in daemon/.
#   4. npm install + npm run build in plugin/.
#   5. Copies the built plugin into $CAPSULE_VAULT_PATH/.obsidian/plugins/capsule-node/
#      via install-plugin.sh (unless --skip-install).
#   6. Prints next steps.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ─── Output helpers ──────────────────────────────────────────────────
if [[ -t 1 ]]; then
  COLOR_INFO=$'\033[1;34m'
  COLOR_OK=$'\033[1;32m'
  COLOR_WARN=$'\033[1;33m'
  COLOR_ERR=$'\033[1;31m'
  COLOR_RESET=$'\033[0m'
else
  COLOR_INFO="" COLOR_OK="" COLOR_WARN="" COLOR_ERR="" COLOR_RESET=""
fi
info() { printf "%s→%s %s\n" "$COLOR_INFO" "$COLOR_RESET" "$*"; }
ok()   { printf "%s✓%s %s\n" "$COLOR_OK"   "$COLOR_RESET" "$*"; }
warn() { printf "%s!%s %s\n" "$COLOR_WARN" "$COLOR_RESET" "$*"; }
err()  { printf "%s✗%s %s\n" "$COLOR_ERR"  "$COLOR_RESET" "$*" >&2; }

# ─── Args ────────────────────────────────────────────────────────────
NON_INTERACTIVE=0
VAULT_ARG=""
SKIP_INSTALL=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --non-interactive) NON_INTERACTIVE=1; shift ;;
    --vault) VAULT_ARG="${2:-}"; shift 2 ;;
    --skip-install) SKIP_INSTALL=1; shift ;;
    -h|--help)
      grep -E '^# ' "$0" | sed 's/^# //'
      exit 0
      ;;
    *)
      err "Unknown argument: $1"
      err "Run with --help for usage."
      exit 1
      ;;
  esac
done

# ─── Prereqs ─────────────────────────────────────────────────────────
require() {
  command -v "$1" >/dev/null 2>&1 || {
    err "$1 is required but not installed. $2"
    exit 1
  }
}
require cargo "Install Rust from https://rustup.rs/"
require rustc "Install Rust from https://rustup.rs/"
require node  "Install Node.js 20+ from https://nodejs.org/"
require npm   "Comes with Node.js"
# jq is needed by e2e-test.sh, not setup.sh itself — warn rather than fail.
if ! command -v jq >/dev/null 2>&1; then
  warn "jq not found. scripts/e2e-test.sh needs it; setup.sh itself does not."
fi

# Node version check (>= 20).
node_major=$(node --version | sed -E 's/^v([0-9]+)\..*/\1/')
if ! [[ "$node_major" =~ ^[0-9]+$ ]] || (( node_major < 20 )); then
  err "Node.js 20+ required; found $(node --version)"
  exit 1
fi
ok "Prereqs OK (rust $(rustc --version | awk '{print $2}'), node $(node --version))"

# ─── .env ───────────────────────────────────────────────────────────
if [[ ! -f .env ]]; then
  if [[ -n "$VAULT_ARG" ]]; then
    VAULT_PATH="$VAULT_ARG"
  elif [[ $NON_INTERACTIVE -eq 1 ]]; then
    err "--non-interactive requires --vault <path> when .env is absent"
    exit 1
  else
    printf "Path to your Obsidian vault: "
    read -r VAULT_PATH
  fi
  if [[ -z "$VAULT_PATH" ]]; then
    err "Vault path cannot be empty"
    exit 1
  fi
  cp .env.example .env
  # Portable in-place edit (BSD sed on macOS needs an empty arg after -i).
  sed "s|^CAPSULE_VAULT_PATH=.*|CAPSULE_VAULT_PATH=${VAULT_PATH//|/\\|}|" .env > .env.tmp
  mv .env.tmp .env
  ok "Wrote .env with CAPSULE_VAULT_PATH=$VAULT_PATH"
else
  info "Using existing .env"
fi

# shellcheck disable=SC1091
set -a; . ./.env; set +a
if [[ -z "${CAPSULE_VAULT_PATH:-}" ]]; then
  err "CAPSULE_VAULT_PATH is empty in .env"
  exit 1
fi

# ─── Build daemon ────────────────────────────────────────────────────
info "Building daemon (cargo build --release)"
(cd daemon && cargo build --release)
ok "Daemon built → daemon/target/release/capsuled"

# ─── Build plugin ────────────────────────────────────────────────────
info "Building plugin (npm install + npm run build)"
(cd plugin && npm install --no-audit --no-fund --silent && npm run build)
ok "Plugin built → plugin/main.js"

# ─── Install into vault ──────────────────────────────────────────────
if [[ $SKIP_INSTALL -eq 1 ]]; then
  info "Skipping plugin install into vault (--skip-install)"
elif [[ ! -d "$CAPSULE_VAULT_PATH" ]]; then
  warn "Vault path \"$CAPSULE_VAULT_PATH\" doesn't exist yet — skipping plugin install."
  warn "Create the vault in Obsidian first, then run: ./scripts/install-plugin.sh"
else
  info "Installing plugin into vault"
  ./scripts/install-plugin.sh
fi

# ─── Done ────────────────────────────────────────────────────────────
cat <<EOF

${COLOR_OK}Setup complete.${COLOR_RESET}

Next steps:
  1. Start the daemon + plugin watcher:
       ./scripts/dev.sh
     or just the daemon:
       cd daemon && cargo run

  2. In Obsidian:
       Settings → Community plugins → enable "Capsule Node"
       Cmd/Ctrl+P → "Capsule Node: Initialize keyring"

  3. Smoke-test the full x402 loop (runs against a disposable temp vault,
     doesn't touch \$CAPSULE_VAULT_PATH):
       ./scripts/e2e-test.sh
EOF
