# Capsule Node

An Obsidian plugin and companion Rust daemon that turn an Obsidian vault into a serving node for the Capsule Protocol — encrypted, policy-gated capsules of personal data that AI agents can query over HTTP with payment (x402) and optional FHE computation.

> **Status:** pre-v0.1. Phase 1 Foundation is complete — the plugin creates capsule notes, the daemon detects them via a filesystem watcher, the node identity is held in an Argon2id-encrypted keyring with mlock'd in-memory key material + auto-lock, and `GET /v1/capsules/{cid}/compute` returns an HTTP 402 signed by a deterministically-derived Ethereum payout address. Phase 2 (real x402 verification + TFHE-rs computation) has not started. APIs and directory layout may still change.

## What works today

- **Capsule notes as source of truth.** Plugin command "Create draft capsule" writes a markdown note with a two-zone YAML frontmatter (user-editable + daemon-managed); edits to the note re-sync the JSON manifest under `.capsule/manifests/`.
- **Filesystem watcher + 402 stub.** Daemon debounces notify events, filters macOS AppleDouble + tempfiles, and registers `cap_*.json` manifests. `GET /v1/capsules/{cid}/compute` returns 402 with a real `recipient` (EIP-55 Ethereum address derived via HKDF-SHA256 → secp256k1) and RFC3339 `expiry`. Returns 503 when the keyring is locked — fail-closed, never serves a request it cannot commit to.
- **Keyring lifecycle.** `POST /api/v1/keyring/{init,unlock,lock}`, with plugin-side passphrase modals. Auto-locks after configurable inactivity (default 30 min, `CAPSULE_KEYRING_AUTO_LOCK_SECS=0` to disable). Argon2id KDF, ChaCha20Poly1305 AEAD, zeroize + mlock in memory, `0600`/`0700` perms on `keyring.enc` and `identity/`.
- **UX in Obsidian.** Status-bar badge + reading-view pill for the active capsule's status. Settings tab shows keyring state, wallet address (selectable + one-click copy), and auto-lock countdown.

## Source of truth

The full architecture, vault layout, daemon API, and build phases are defined in [`capsule-node-spec.md`](./capsule-node-spec.md). Read that before proposing changes.

## Repo layout

- [`daemon/`](./daemon) — `capsuled`, the Rust companion daemon (axum + tokio).
- [`plugin/`](./plugin) — the TypeScript Obsidian plugin.
- [`scripts/`](./scripts) — `setup.sh`, `dev.sh`, `e2e-test.sh`, `install-plugin.sh`.
- [`.github/workflows/ci.yml`](./.github/workflows/ci.yml) — CI for both tracks.
- [`CLAUDE.md`](./CLAUDE.md) — architectural invariants and security posture for contributors (and AI assistants) touching this code.

## Quick start

```bash
# One-shot: builds both halves, installs the plugin into your vault,
# prompts for the vault path if .env is missing.
./scripts/setup.sh

# Then run the daemon + plugin watcher in parallel:
./scripts/dev.sh
```

In Obsidian: enable **Capsule Node** under Settings → Community plugins, then **Cmd/Ctrl+P → "Capsule Node: Initialize keyring"** to set a passphrase.

### Verify the full loop

```bash
./scripts/e2e-test.sh
```

Spins the daemon up in a disposable temp vault on ephemeral ports (won't collide with your dev daemon) and walks through init → manifest → 402 → lock → 503. Exits non-zero on the first mismatch. Requires `jq`.

### Manual run (no setup script)

```bash
cp .env.example .env
$EDITOR .env        # set CAPSULE_VAULT_PATH
cd daemon && cargo run
# in another terminal:
cd plugin && npm install && npm run dev
curl http://127.0.0.1:7402/api/v1/status
```

## Contributing

Every change must preserve the security invariants listed in `CLAUDE.md` — in particular: the management API binds to `127.0.0.1` only, `.capsule/` stays `0700`, no `Debug`/`Display` impls may leak key material, and no secrets, keys, or maintainer-specific paths enter the repo. Before opening a PR:

```bash
# daemon
cd daemon && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
# plugin
cd plugin && npm run lint && npm run build && npm run test
# end-to-end
./scripts/e2e-test.sh
```

## License

[MIT](./LICENSE).
