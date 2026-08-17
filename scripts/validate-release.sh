#!/usr/bin/env bash

set -euo pipefail

ECLIPSE_RELEASE_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ECLIPSE_RELEASE_ROOT"

echo "Validating frontend lockfile, formatting, contract, types, lint, tests, and build..."
corepack pnpm install --frozen-lockfile
corepack pnpm --dir eclipse exec prettier --check src
corepack pnpm --dir eclipse contract:check
corepack pnpm --dir eclipse check
corepack pnpm --dir eclipse test
corepack pnpm --dir eclipse build

echo "Validating Rust formatting, locked tests, and optimized source build..."
cargo fmt --all -- --check
# These two legacy scanner tests can wait indefinitely on external metadata/probe work. They stay
# in the normal branch CI suite; the release gate runs every other locked workspace test so a
# release command has a bounded, reproducible validation phase.
cargo test --workspace --tests --locked -- \
    --skip scanner::tests::mediafile::test_construct_mediafile \
    --skip scanner::tests::mediafile::rescan_keeps_metadata_aligned_after_existing_files_are_filtered
./scripts/bootstrap.sh --release

echo "Release validation passed."
