# Capsule Node

An Obsidian plugin and companion Rust daemon that turn an Obsidian vault into a serving node for the Capsule Protocol — encrypted, policy-gated capsules of personal data that AI agents can query over HTTP with payment (x402) and optional FHE computation.

> **Status:** pre-v0.1 walking skeleton. The daemon boots and serves `/api/v1/status` and `/v1/node/info`; the plugin loads in Obsidian and pings the daemon. No manifests, no payments, no cryptography yet. APIs and directory layout will change.

## Source of truth

The full architecture, vault layout, daemon API, and build phases are defined in [`capsule-node-spec.md`](./capsule-node-spec.md). Read that before proposing changes.

## Repo layout

- [`daemon/`](./daemon) — `capsuled`, the Rust companion daemon (axum + tokio).
- [`plugin/`](./plugin) — the TypeScript Obsidian plugin.
- [`.github/workflows/ci.yml`](./.github/workflows/ci.yml) — CI for both tracks.
- [`CLAUDE.md`](./CLAUDE.md) — architectural invariants and security posture for contributors (and AI assistants) touching this code.

## Quick start

```bash
# 1. Configure local env
cp .env.example .env
$EDITOR .env        # set CAPSULE_VAULT_PATH to your Obsidian vault

# 2. Run both halves (in separate terminals, or use scripts/dev.sh)
cd daemon && cargo run
cd plugin && npm install && npm run dev

# 3. Verify the daemon is up
curl http://127.0.0.1:7402/api/v1/status
```

To use the plugin inside Obsidian, symlink `plugin/` into `<vault>/.obsidian/plugins/capsule-node/` and enable it in Obsidian's Community Plugins settings.

## Contributing

Every change must preserve the security invariants listed in `CLAUDE.md` — in particular: the management API binds to `127.0.0.1` only, `.capsule/` stays `0700`, and no secrets, keys, or maintainer-specific paths enter the repo. Run `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, `npm run lint`, and `npm run build` before opening a PR.

## License

[MIT](./LICENSE).
