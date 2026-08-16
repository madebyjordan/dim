#!/usr/bin/env bash

set -euo pipefail

DIM_ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

cd "$DIM_ROOT_DIR"

echo "Running release command tests..."
node --test ./scripts/release.test.mjs

echo "Running frontend tests with the UI's pinned Yarn version..."
(
    cd ui
    corepack yarn test
)

echo "Running locked Rust workspace tests..."
# These legacy scanner tests can wait indefinitely on external metadata/probe work. Normal branch
# CI still runs them; keep the local root command deterministic and aligned with the release gate.
cargo test --workspace --tests --locked -- \
    --skip scanner::tests::mediafile::test_construct_mediafile \
    --skip scanner::tests::mediafile::rescan_keeps_metadata_aligned_after_existing_files_are_filtered
