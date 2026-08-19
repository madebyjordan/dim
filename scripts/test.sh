#!/usr/bin/env bash

set -euo pipefail

DIM_ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

cd "$DIM_ROOT_DIR"

echo "Running installer and release command tests..."
node --test ./scripts/generate-api-contract.test.mjs ./scripts/install.test.mjs ./scripts/package-manager.test.mjs ./scripts/rust-dependencies.test.mjs ./scripts/windows-launcher.test.mjs ./scripts/windows-scripts.test.mjs ./scripts/windows-toolchain.test.mjs ./scripts/release.test.mjs

echo "Running Eclipse frontend tests..."
corepack pnpm --dir eclipse test
corepack pnpm --dir eclipse check

echo "Running locked Rust workspace tests..."
# These legacy scanner tests can wait indefinitely on external metadata/probe work. Normal branch
# CI still runs them; keep the local root command deterministic and aligned with the release gate.
cargo test --workspace --tests --locked -- \
    --skip scanner::tests::mediafile::test_construct_mediafile \
    --skip scanner::tests::mediafile::rescan_keeps_metadata_aligned_after_existing_files_are_filtered
