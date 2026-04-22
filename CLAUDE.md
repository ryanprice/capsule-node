# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository status

This repo is **pre-implementation**. It currently contains a single file: `capsule-node-spec.md` (the v0.1 draft of the Capsule Node technical specification). There is no source code, build system, package manifest, test suite, or CI yet. Before scaffolding anything new, read the spec — it is the source of truth for every architectural decision listed below.

When asked to implement something, the spec's "Phase 1–4" sequence in Section 9 defines the intended build order. Don't jump ahead of that sequence without checking with the user.

## System architecture (two-process, filesystem-coupled)

Capsule Node is deliberately split into two independently-deployable processes that share the Obsidian vault as their communication channel:

- **Obsidian plugin** (TypeScript, community plugin) — the **management layer**. Human-facing. Validates capsule frontmatter, encrypts payloads, compiles policies, renders dashboards, controls the daemon.
- **Companion daemon `capsuled`** (Rust, tokio/Axum) — the **serving layer**. Machine-facing. Serves the public HTTP API to AI agents, processes x402 payments, executes FHE computations (TFHE-rs), manages registry presence.

**Neither component depends on the other to function in its primary role.** The plugin can be uninstalled/reinstalled without losing state; the daemon can run headless on a VPS or Raspberry Pi without Obsidian present. Any implementation that couples them (shared in-process state, required startup ordering, RPC-only state) violates this invariant.

### The two communication channels

1. **Filesystem (primary, persistent state)** — the `.capsule/` directory inside the vault. Plugin writes configs, manifests, compiled policies, encrypted payloads. Daemon writes activity logs and earnings summaries. Both watch for changes via OS-native watchers (inotify / FSEvents / ReadDirectoryChangesW).
2. **Localhost HTTP (secondary, ephemeral ops)** — `127.0.0.1:{daemon_port}` (default 7402). Used only for real-time operations the filesystem can't express efficiently: start/stop, live status, bid approvals, active connection lists.

Rule of thumb when deciding where a new piece of state belongs: **persistent state → filesystem; ephemeral/live-query → localhost API.**

### Two HTTP surfaces, not one

Do not conflate them:

- `external_port` (default 8402) — **public, agent-facing**, TLS required, implements x402 payment flow, must be rate-limited and DDoS-resistant.
- `daemon_port` (default 7402) — **localhost-only, plugin-facing**, no TLS, trusted caller.

## Vault layout as the protocol

The `.capsule/` hidden directory is the contract between plugin and daemon. Every subdirectory has a defined owner:

| Path | Writer | Reader |
|---|---|---|
| `.capsule/config.yaml` | plugin (via SettingsPanel) | daemon |
| `.capsule/identity/` | plugin (setup wizard) | daemon |
| `.capsule/payloads/{cid}.enc` | plugin (CapsuleManager) | daemon (FHE workers) |
| `.capsule/manifests/{cid}.json` | plugin writes, daemon updates computed fields | both |
| `.capsule/policies/{pol}.json` | plugin (PolicyCompiler) | daemon |
| `.capsule/activity/{cid}.jsonl` | daemon (append-only) | plugin (dashboards) |
| `.capsule/activity/earnings_summary.json` | daemon (rolling update) | plugin |
| `.capsule/runtime/computation_circuits/` | daemon (fetched from schema registry) | daemon |
| `.capsule/daemon.pid` | daemon | plugin (liveness check) |

Activity logs are **JSONL, append-only** — never rewrite them. Manifests are JSON objects that both sides edit, but with a strict field-ownership boundary (see below).

## The user-editable vs daemon-managed boundary

Capsule note YAML frontmatter has two zones separated by the `# ═══ Computed Fields (daemon-managed, read-only) ═══` comment:

- **Above the comment:** user-editable (pricing, status, tags, policy links, credentials, data profile). The plugin validates these on save.
- **Below the comment:** daemon-managed (`payload_cid`, `earnings_total`, `queries_served`, `last_accessed`, `reputation`). The plugin must never write these; the daemon updates them via manifest writes that the plugin detects and mirrors into the note.

When editing frontmatter logic on either side, preserve this boundary. Crossing it from the wrong side creates race conditions and silently loses user edits.

## Request processing pipeline (daemon)

Agent requests flow through a fixed 8-stage pipeline (spec §4.3): **Parse → Manifest Lookup → Payment Check (402 if missing) → Payment Verify → Policy Evaluate → Computation Dispatch → Result Return → Audit Log.** Do not reorder stages. In particular: payment is verified *before* policy evaluation, and the audit log write is the final step (so failed computations don't produce spurious revenue records).

## FHE circuit integrity

Computation circuits under `.capsule/runtime/computation_circuits/` are versioned and hash-checked against the on-chain schema registry. The daemon **must refuse** to execute a circuit whose SHA-256 does not match the registry entry. Any circuit-loading code must fail closed, not open.

## Key material handling (daemon, Rust)

When implementing anything that touches the keyring:
- Keys live encrypted at rest in `.capsule/identity/keyring.enc` (Argon2id KDF from passphrase).
- Decrypted key material must be `mlock()`/`VirtualLock()`'d to prevent swap.
- Use the `zeroize` crate for all in-memory key types.
- Auto-lock after configured inactivity (default 30 min) — post-lock, daemon continues running but refuses computations until re-auth.

## Key technology choices (don't swap without discussion)

| Area | Choice | Why it matters |
|---|---|---|
| Daemon language | Rust | FHE is CPU-bound; also: single static binary, memory safety for key handling |
| Async runtime | tokio | Spec-mandated; Axum HTTP framework is built on it |
| FHE | TFHE-rs | Rust-native, lattice-based (quantum-resistant) |
| Payment | x402 + USDC on Base | Sub-cent fees, agent-native |
| Plugin language | TypeScript | Obsidian's native plugin language |
| Activity log | JSONL | Append-only, no DB dependency, greppable |
| Policy format | YAML in policy notes, compiled to JSON | Policies are human-readable documents with wikilinks, not opaque blobs |

## Open-source security posture

**This repo is public.** Every change must be written as if a stranger will read it tomorrow. Treat any leak of secrets or user data as a production incident — `git rm` does not erase history.

### What must never enter the repo

- `.env` files with real values (only `.env.example` is tracked)
- Private keys, mnemonics, seed phrases, wallet files (`*.key`, `*.pem`, `keyring*`, `wallet.json`, `*.mnemonic`)
- Contents of anyone's `.capsule/` directory (payloads, activity logs, identity, credentials)
- Absolute paths, usernames, DIDs, or wallet addresses from the maintainer's machine in source, tests, fixtures, or docs
- Real user data in test fixtures — use synthetic data generators, not redacted real exports
- API tokens for any facilitator, indexer, or third-party service

The `.gitignore` blocks the common shapes of these, but defense-in-depth matters: review every `git add` before commit. When pasting logs or snippets into issues/PRs, scrub paths and IDs.

### Configuration, not hardcoding

Anything machine-specific must come from env or config, never a literal in source:

- **Vault path** → `CAPSULE_VAULT_PATH` (see `.env.example`). The daemon's `--vault` flag and the plugin's settings both resolve from this. No code should contain the maintainer's local path.
- **Ports, network, wallet address** → env vars with sane defaults in `.env.example`.
- **Keyring** → stays encrypted in `$CAPSULE_VAULT_PATH/.capsule/identity/keyring.enc`. It is decrypted into mlock'd memory at daemon startup. The env file configures *which* wallet, never the key itself.

Set your local vault path in `.env` (which is gitignored). Never put it in committed code, fixtures, or docs — `.env.example` should always have an empty `CAPSULE_VAULT_PATH=` placeholder.

### Security invariants the implementation must preserve

These are already in spec §9, but worth restating so they aren't accidentally weakened in PRs:

- **Fail closed, not open.** Circuit hash mismatch → refuse. Policy evaluation error → deny. Payment verification timeout → drop request. Never default to permit on error.
- **Localhost API stays localhost.** The management API on `127.0.0.1:{daemon_port}` must bind to the loopback interface only — never `0.0.0.0`. If a PR adds auth to it, that's a smell: it should be unreachable from the network instead.
- **Public API requires TLS.** No `--insecure` flag, no self-signed fallback in prod builds (self-signed is allowed for local dev only, gated behind an explicit flag).
- **Key material uses `zeroize` and `mlock`/`VirtualLock`.** Any new Rust type that holds a secret derives `Zeroize` + `ZeroizeOnDrop` and is locked in memory. Do not add `Debug` or `Display` impls that would print secrets.
- **Vault file permissions.** `.capsule/` is `0700`, payload files are `0600` on Unix; NTFS ACLs restrict to current user on Windows. Code that creates files in `.capsule/` must set these perms explicitly — don't rely on umask.
- **Rate limits are not optional.** Per-agent, global, queue-depth, and payment-verification timeouts all ship in the first serving release (spec §9.3).
- **Input validation on the public API.** Every field from an agent request is untrusted: parse, bound-check, and reject oversized payloads early. The daemon is exposed to the open internet.

### Pre-commit checklist

Before any commit:

1. `git diff --staged` — eyeball for paths, addresses, tokens, DIDs that shouldn't be there.
2. Confirm `.env` is not staged (it's gitignored, but double-check after `git add -A`).
3. Any new file that touches crypto, payments, policy evaluation, or the public API gets a second read specifically for the "fail closed" property.

When adding a dependency, check its license compatibility (we'll pick a license before first public release — until then, assume MIT/Apache-2.0-compatible only) and its maintenance status. Avoid pulling in un-audited crypto crates.

## Build / test / run commands

### Daemon (`daemon/`, Rust)

```bash
cd daemon
cargo run                                   # boots daemon; reads $CAPSULE_VAULT_PATH from .env
cargo test                                  # unit + smoke tests (endpoints_respond_on_both_surfaces, prepare_vault_*)
cargo fmt --all -- --check                  # formatting check (CI-equivalent)
cargo clippy --all-targets -- -D warnings   # lint (CI-equivalent, treats warnings as errors)
cargo build --release                       # production binary at target/release/capsuled
```

Single-test run: `cargo test endpoints_respond_on_both_surfaces -- --exact`.

The library lives in `daemon/src/lib.rs` so integration tests in `daemon/tests/` can call `capsuled::serve(...)` with pre-bound ephemeral listeners — keep this pattern when adding new endpoints.

### Plugin (`plugin/`, TypeScript)

```bash
cd plugin
npm install       # first time only
npm run dev       # esbuild watch — rebuilds main.js on save
npm run build     # tsc --noEmit + production esbuild bundle → main.js
npm run lint      # eslint (flat config at eslint.config.mjs)
```

To use the plugin inside Obsidian during development: symlink `plugin/` into `<vault>/.obsidian/plugins/capsule-node/` and enable it in Community Plugins. The built `main.js` is gitignored — only `src/`, `manifest.json`, and the build configs are tracked.

### Full local loop

`scripts/dev.sh` sources `.env`, runs `npm run dev` and `cargo run` in parallel, and cleans both up on Ctrl-C. Requires `.env` with `CAPSULE_VAULT_PATH` set.

### One-shot setup

`scripts/setup.sh` is the clone-and-go entry point. Checks prereqs (cargo, node ≥ 20, npm), prompts for the vault path if `.env` is missing, builds both halves (`cargo build --release`, `npm install && npm run build`), and runs `install-plugin.sh` to copy the built plugin into the vault. Supports `--vault <path>` and `--non-interactive` for CI. Run `./scripts/setup.sh --help` for the full flag list.

### End-to-end smoke

`scripts/e2e-test.sh` spins the daemon up against a disposable temp vault on ephemeral ports (17402 / 18402, so it won't collide with a running dev daemon), walks through init → manifest → 402 → lock → 503, and tears everything down. Exits non-zero on the first mismatch. Requires `jq`. Uses `daemon/target/release/capsuled` if present, falls back to the debug binary. Safe to run repeatedly.

### What's not here yet

The spec's Phase 1 Foundation milestone (plugin creates capsule note → daemon detects → endpoint returns HTTP 402) builds on this walking skeleton by adding the filesystem watcher, manifest parser, and x402 stub. Those are the next features, not refactors — don't rework the current scaffolding, extend it.
