#!/usr/bin/env bash

set -euo pipefail

DIM_RELEASE_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$DIM_RELEASE_ROOT"

echo "Validating frontend lockfile, formatting, contract, types, lint, tests, and build..."
(
    cd ui
    corepack yarn install --immutable --mode=skip-build
    corepack yarn prettier --check --ignore-unknown src
    corepack yarn contract:check
    corepack yarn typecheck
    corepack yarn lint
    corepack yarn test
    corepack yarn build
)

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
