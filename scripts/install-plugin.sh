#!/usr/bin/env bash
# Copy the built plugin (main.js + manifest.json + styles.css if present) into
# the Obsidian vault's plugins directory. Run after `npm run build` in plugin/.
#
# Reads CAPSULE_VAULT_PATH from .env at the repo root.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if [[ ! -f .env ]]; then
  echo "error: .env not found at $REPO_ROOT" >&2
  exit 1
fi

# shellcheck disable=SC1091
set -a; . ./.env; set +a

if [[ -z "${CAPSULE_VAULT_PATH:-}" ]]; then
  echo "error: CAPSULE_VAULT_PATH not set in .env" >&2
  exit 1
fi

SRC="$REPO_ROOT/plugin"
DEST="$CAPSULE_VAULT_PATH/.obsidian/plugins/capsule-node"

if [[ ! -f "$SRC/main.js" ]]; then
  echo "error: $SRC/main.js not found — run 'npm run build' in plugin/ first" >&2
  exit 1
fi

mkdir -p "$DEST"
# If the destination is a broken symlink (e.g. from a prior dev setup), clear it.
if [[ -L "$DEST" && ! -e "$DEST" ]]; then
  rm "$DEST"
  mkdir -p "$DEST"
fi

cp "$SRC/manifest.json" "$DEST/manifest.json"
cp "$SRC/main.js" "$DEST/main.js"
if [[ -f "$SRC/styles.css" ]]; then
  cp "$SRC/styles.css" "$DEST/styles.css"
fi

echo "installed plugin → $DEST"
ls -la "$DEST"
