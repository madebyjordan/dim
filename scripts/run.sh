#!/usr/bin/env bash

set -euo pipefail

ECLIPSE_ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
ECLIPSE_PROFILE=debug
ECLIPSE_RUNTIME_DIR="$ECLIPSE_ROOT_DIR"
ECLIPSE_BINARY_SUFFIX=""

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) ECLIPSE_BINARY_SUFFIX=".exe" ;;
esac

if [[ "${1:-}" == "--release" ]]; then
    ECLIPSE_PROFILE=release
    shift
fi

ECLIPSE_CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$ECLIPSE_ROOT_DIR/target"}
if [[ "$ECLIPSE_CARGO_TARGET_DIR" != /* ]]; then
    ECLIPSE_CARGO_TARGET_DIR="$ECLIPSE_ROOT_DIR/$ECLIPSE_CARGO_TARGET_DIR"
fi

ECLIPSE_BINARY="$ECLIPSE_CARGO_TARGET_DIR/$ECLIPSE_PROFILE/eclipse$ECLIPSE_BINARY_SUFFIX"
if [[ "$ECLIPSE_PROFILE" == release ]]; then
    ECLIPSE_RUNTIME_DIR="$ECLIPSE_CARGO_TARGET_DIR/release"
fi

if [[ ! -x "$ECLIPSE_BINARY" ]]; then
    ECLIPSE_BOOTSTRAP_SUFFIX=""
    if [[ "$ECLIPSE_PROFILE" == release ]]; then
        ECLIPSE_BOOTSTRAP_SUFFIX=" --release"
    fi
    echo "Eclipse has not been built at $ECLIPSE_BINARY." >&2
    echo "Run corepack pnpm build$ECLIPSE_BOOTSTRAP_SUFFIX first." >&2
    exit 1
fi

cd "$ECLIPSE_RUNTIME_DIR"
exec "$ECLIPSE_BINARY" "$@"
