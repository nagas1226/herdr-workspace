#!/usr/bin/env bash
# Generate CHANGELOG.md from conventional commits with git-cliff.
#
#   scripts/changelog.sh                 write CHANGELOG.md (all tagged releases)
#   scripts/changelog.sh --unreleased    preview commits since the latest tag
#   scripts/changelog.sh --latest        preview the latest tagged release
#   scripts/changelog.sh --tag v0.1.0    write CHANGELOG.md as if that tag exists
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v git-cliff >/dev/null 2>&1; then
  echo "changelog: git-cliff is not installed." >&2
  echo "  brew install git-cliff" >&2
  echo "  # or: cargo install git-cliff" >&2
  exit 1
fi

config="$ROOT/cliff.toml"
case "${1:-}" in
  --unreleased)
    git-cliff --config "$config" --unreleased --strip header
    ;;
  --latest)
    git-cliff --config "$config" --latest --strip header
    ;;
  --tag)
    tag="${2:?usage: $0 --tag vX.Y.Z}"
    git-cliff --config "$config" --tag "$tag" -o "$ROOT/CHANGELOG.md"
    echo "wrote $ROOT/CHANGELOG.md for $tag"
    ;;
  "")
    git-cliff --config "$config" -o "$ROOT/CHANGELOG.md"
    echo "wrote $ROOT/CHANGELOG.md"
    ;;
  *)
    echo "usage: $0 [--unreleased|--latest|--tag vX.Y.Z]" >&2
    exit 2
    ;;
esac
