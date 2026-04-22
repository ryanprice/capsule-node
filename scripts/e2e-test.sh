#!/usr/bin/env bash
# Capsule Node — end-to-end smoke test.
#
# Walks through the full Phase 1 loop against a disposable temp vault:
#   1. Boot daemon with no keyring        → /api/v1/status says "none"
#   2. POST /api/v1/keyring/init          → "unlocked", wallet_address appears
#   3. Write a capsule manifest           → /v1/capsules lists it
#   4. GET  /v1/capsules/{cid}/compute    → 402 with real recipient + expiry
#   5. POST /api/v1/keyring/lock          → "locked"
#   6. GET  /v1/capsules/{cid}/compute    → 503 (fail-closed)
#
# Exits 0 on success, non-zero on first failure. Tears down the daemon + vault
# even on failure. Safe to run against a machine already running a dev daemon
# — uses ephemeral ports (17402/18402) that won't collide with 7402/8402.
#
# Usage: ./scripts/e2e-test.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ─── Output helpers ──────────────────────────────────────────────────
if [[ -t 1 ]]; then
  COLOR_INFO=$'\033[1;34m'
  COLOR_OK=$'\033[1;32m'
  COLOR_ERR=$'\033[1;31m'
  COLOR_DIM=$'\033[2m'
  COLOR_RESET=$'\033[0m'
else
  COLOR_INFO="" COLOR_OK="" COLOR_ERR="" COLOR_DIM="" COLOR_RESET=""
fi
info() { printf "%s→%s %s\n" "$COLOR_INFO" "$COLOR_RESET" "$*"; }
ok()   { printf "%s✓%s %s\n" "$COLOR_OK"   "$COLOR_RESET" "$*"; }
err()  { printf "%s✗%s %s\n" "$COLOR_ERR"  "$COLOR_RESET" "$*" >&2; }
dim()  { printf "%s%s%s\n" "$COLOR_DIM" "$*" "$COLOR_RESET"; }

# ─── Prereqs ─────────────────────────────────────────────────────────
for cmd in jq curl; do
  command -v "$cmd" >/dev/null 2>&1 || {
    err "$cmd is required for the e2e test"
    exit 1
  }
done

BINARY="daemon/target/release/capsuled"
if [[ ! -x "$BINARY" ]]; then
  # Fall back to debug build if release isn't present — lets a freshly
  # cargo-built tree pass without requiring a separate --release build.
  BINARY="daemon/target/debug/capsuled"
  if [[ ! -x "$BINARY" ]]; then
    err "No capsuled binary found. Run 'cargo build' in daemon/ first (or ./scripts/setup.sh)."
    exit 1
  fi
fi

# ─── State ───────────────────────────────────────────────────────────
VAULT=$(mktemp -d -t capsuled-e2e.XXXXXX)
MGMT_PORT=17402
PUBLIC_PORT=18402
PASSPHRASE="e2e-test-throwaway-$$"
CID="cap_e2etest"
LOG="$VAULT/daemon.log"
DAEMON_PID=""

cleanup() {
  local exit_code=$?
  if [[ -n "$DAEMON_PID" ]] && kill -0 "$DAEMON_PID" 2>/dev/null; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  if (( exit_code != 0 )); then
    err "FAILED — daemon log:"
    echo "${COLOR_DIM}$(sed 's/^/    /' "$LOG" 2>/dev/null || echo '    (no log)')${COLOR_RESET}"
  fi
  rm -rf "$VAULT"
  exit $exit_code
}
trap cleanup EXIT INT TERM

info "Temp vault: $VAULT"
info "Daemon binary: $BINARY"

# ─── Start daemon ────────────────────────────────────────────────────
info "Starting daemon (mgmt=$MGMT_PORT public=$PUBLIC_PORT)"
(
  export CAPSULE_VAULT_PATH="$VAULT"
  export CAPSULE_DAEMON_PORT="$MGMT_PORT"
  export CAPSULE_EXTERNAL_PORT="$PUBLIC_PORT"
  export CAPSULE_LOG_LEVEL="warn"
  # Disable auto-lock for the test so the lock transition is deterministic.
  export CAPSULE_KEYRING_AUTO_LOCK_SECS="0"
  exec "$BINARY"
) >"$LOG" 2>&1 &
DAEMON_PID=$!

# Poll until /status responds, up to 10s.
for _ in $(seq 1 40); do
  if curl -sf "http://127.0.0.1:$MGMT_PORT/api/v1/status" >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done
if ! curl -sf "http://127.0.0.1:$MGMT_PORT/api/v1/status" >/dev/null 2>&1; then
  err "Daemon did not respond within 10s"
  exit 1
fi
ok "Daemon up"

# Helpers.
mgmt_url() { echo "http://127.0.0.1:$MGMT_PORT$1"; }
pub_url()  { echo "http://127.0.0.1:$PUBLIC_PORT$1"; }

assert_eq() {
  local got="$1" want="$2" label="$3"
  if [[ "$got" != "$want" ]]; then
    err "$label: expected \"$want\", got \"$got\""
    exit 1
  fi
}

# ─── 1. Boot state ───────────────────────────────────────────────────
info "[1/6] Boot state: keyring should be \"none\""
STATE=$(curl -s "$(mgmt_url /api/v1/status)" | jq -r '.keyring')
assert_eq "$STATE" "none" "initial keyring state"
ok "Boot: keyring=none"

# ─── 2. Init keyring ─────────────────────────────────────────────────
info "[2/6] Initializing keyring (Argon2id, may take ~100ms)"
RESP=$(curl -s -X POST "$(mgmt_url /api/v1/keyring/init)" \
  -H "content-type: application/json" \
  -d "$(jq -cn --arg p "$PASSPHRASE" '{passphrase:$p}')")
STATE=$(echo "$RESP" | jq -r '.status // "missing"')
assert_eq "$STATE" "unlocked" "post-init keyring state"
ADDR=$(curl -s "$(mgmt_url /api/v1/status)" | jq -r '.wallet_address // ""')
if ! [[ "$ADDR" =~ ^0x[a-fA-F0-9]{40}$ ]]; then
  err "wallet_address missing or malformed: \"$ADDR\""
  exit 1
fi
ok "Init: wallet_address=$ADDR"

# ─── 3. Register a manifest ──────────────────────────────────────────
info "[3/6] Writing manifest; waiting for watcher to pick it up"
mkdir -p "$VAULT/.capsule/manifests"
cat > "$VAULT/.capsule/manifests/$CID.json" <<JSON
{
  "capsule_id": "$CID",
  "schema": "capsule://e2e.test",
  "status": "active",
  "floor_price": "0.05 USDC/query",
  "computation_classes": ["A"],
  "tags": ["e2e"]
}
JSON
# Watcher debounce is 150ms; poll up to 3s.
for _ in $(seq 1 30); do
  COUNT=$(curl -s "$(pub_url /v1/capsules)" | jq "length")
  if [[ "$COUNT" == "1" ]]; then
    break
  fi
  sleep 0.1
done
COUNT=$(curl -s "$(pub_url /v1/capsules)" | jq "length")
assert_eq "$COUNT" "1" "active capsule count"
FOUND=$(curl -s "$(pub_url /v1/capsules)" | jq -r '.[0].capsule_id')
assert_eq "$FOUND" "$CID" "registered capsule id"
ok "Daemon registered $CID"

# ─── 4. 402 on unlocked ──────────────────────────────────────────────
info "[4/6] GET /compute while unlocked → expect 402 with recipient=wallet"
HTTP=$(curl -s -o "$VAULT/compute.json" -w "%{http_code}" "$(pub_url "/v1/capsules/$CID/compute")")
assert_eq "$HTTP" "402" "compute HTTP status (unlocked)"
RECIPIENT=$(jq -r '.recipient // ""' "$VAULT/compute.json")
assert_eq "$RECIPIENT" "$ADDR" "402 recipient == wallet_address"
EXPIRY=$(jq -r '.expiry // ""' "$VAULT/compute.json")
if [[ -z "$EXPIRY" ]]; then
  err "402 response missing expiry"
  exit 1
fi
ok "402: recipient + expiry populated ($EXPIRY)"

# ─── 5. Lock ─────────────────────────────────────────────────────────
info "[5/6] POST /api/v1/keyring/lock"
RESP=$(curl -s -X POST "$(mgmt_url /api/v1/keyring/lock)")
STATE=$(echo "$RESP" | jq -r '.status // "missing"')
assert_eq "$STATE" "locked" "post-lock keyring state"
ok "Lock: keyring=locked"

# ─── 6. 503 on locked (fail-closed) ──────────────────────────────────
info "[6/6] GET /compute while locked → expect 503 (fail-closed)"
HTTP=$(curl -s -o /dev/null -w "%{http_code}" "$(pub_url "/v1/capsules/$CID/compute")")
assert_eq "$HTTP" "503" "compute HTTP status (locked)"
ok "Locked: /compute returns 503"

printf "\n%sAll e2e checks passed.%s\n" "$COLOR_OK" "$COLOR_RESET"
