#!/usr/bin/env bash
# herdr `[[build]]` step: build this checkout into bin/. This fork contains
# local changes that are not part of an upstream release, so downloading the
# upstream release binary would silently discard them.
set -euo pipefail

NAME="herdr-workspace"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DIR="$ROOT/bin"

echo "$NAME: building fork source"
cargo build --release --manifest-path "$ROOT/Cargo.toml"
mkdir -p "$BIN_DIR"
install -m 0755 "$ROOT/target/release/$NAME" "$BIN_DIR/$NAME"
echo "$NAME: installed $BIN_DIR/$NAME"
