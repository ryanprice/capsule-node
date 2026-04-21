#!/usr/bin/env bash
# Local dev convenience: run the daemon and watch the plugin build in parallel.
# Ctrl-C stops both. Requires: cargo, npm, a populated .env at the repo root.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if [[ ! -f .env ]]; then
  echo "error: .env not found at $REPO_ROOT — copy .env.example and set CAPSULE_VAULT_PATH" >&2
  exit 1
fi

# shellcheck disable=SC1091
set -a; . ./.env; set +a

if [[ -z "${CAPSULE_VAULT_PATH:-}" ]]; then
  echo "error: CAPSULE_VAULT_PATH is not set in .env" >&2
  exit 1
fi

pids=()
cleanup() {
  for pid in "${pids[@]:-}"; do
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
  done
  wait || true
}
trap cleanup EXIT INT TERM

(cd plugin && npm run dev) &
pids+=($!)

(cd daemon && cargo run) &
pids+=($!)

wait -n "${pids[@]}"
