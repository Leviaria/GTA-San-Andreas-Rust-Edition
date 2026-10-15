#!/usr/bin/env bash
# macOS launcher: build when Cargo is present, then run with Metal renderer.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
if command -v cargo >/dev/null 2>&1; then
    cargo build --manifest-path native/Cargo.toml --release --workspace
fi
if [[ ! -x native/target/release/sa-launcher ]]; then
    echo "sa-launcher is missing. Install Rust/Cargo from https://rustup.rs/" >&2
    exit 1
fi
if [[ -n "${GTA_SA_DIR:-}" ]]; then
    exec native/target/release/sa-launcher --renderer auto --game-dir "$GTA_SA_DIR" "$@"
fi
exec native/target/release/sa-launcher --renderer auto "$@"
