#!/usr/bin/env bash

set -euo pipefail

DIM_ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DIM_PROFILE=debug
DIM_RUNTIME_DIR="$DIM_ROOT_DIR"

if [[ "${1:-}" == "--release" ]]; then
    DIM_PROFILE=release
    shift
fi

DIM_CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$DIM_ROOT_DIR/target"}
if [[ "$DIM_CARGO_TARGET_DIR" != /* ]]; then
    DIM_CARGO_TARGET_DIR="$DIM_ROOT_DIR/$DIM_CARGO_TARGET_DIR"
fi

DIM_BINARY="$DIM_CARGO_TARGET_DIR/$DIM_PROFILE/dim"
if [[ "$DIM_PROFILE" == release ]]; then
    DIM_RUNTIME_DIR="$DIM_CARGO_TARGET_DIR/release"
fi

if [[ ! -x "$DIM_BINARY" ]]; then
    DIM_BOOTSTRAP_SUFFIX=""
    if [[ "$DIM_PROFILE" == release ]]; then
        DIM_BOOTSTRAP_SUFFIX=" --release"
    fi
    echo "Dim has not been built at $DIM_BINARY." >&2
    echo "Run yarn build$DIM_BOOTSTRAP_SUFFIX first." >&2
    exit 1
fi

cd "$DIM_RUNTIME_DIR"
exec "$DIM_BINARY" "$@"
