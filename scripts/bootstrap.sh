#!/usr/bin/env bash

set -euo pipefail

DIM_ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
ECLIPSE_RELEASE_BUILD=false
DIM_SKIP_UI=false
DIM_SKIP_RUST=false

usage() {
    echo "Usage: ./scripts/bootstrap.sh [--release] [--skip-ui] [--skip-rust]"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --release)
            ECLIPSE_RELEASE_BUILD=true
            ;;
        --skip-ui)
            DIM_SKIP_UI=true
            ;;
        --skip-rust)
            DIM_SKIP_RUST=true
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Missing required command: $1" >&2
        exit 1
    fi
}

require_command git
require_command ffmpeg
require_command ffprobe

if [[ "$DIM_SKIP_UI" == false ]]; then
    require_command node
    require_command corepack

    if ! node -e '
        const [major, minor, patch] = process.versions.node.split(".").map(Number);
        process.exit(major === 24 && (minor > 19 || (minor === 19 && patch >= 0)) ? 0 : 1);
    '; then
        echo "Dim requires Node.js 24.19.0 or newer in the 24.x line; found $(node --version)." >&2
        exit 1
    fi
fi

if [[ "$DIM_SKIP_RUST" == false ]]; then
    require_command cargo
    require_command rustc
fi

cd "$DIM_ROOT_DIR"
mkdir -p utils

link_media_tool() {
    local name=$1
    local destination=$2
    local source_path
    source_path=$(command -v "$name")

    if [[ -e "$destination" || -L "$destination" ]]; then
        if [[ ! -x "$destination" ]]; then
            echo "$destination exists but is not executable; leaving it unchanged." >&2
            exit 1
        fi
        echo "Using existing $destination"
    else
        ln -s "$source_path" "$destination"
    fi
}

link_media_tool ffmpeg "utils/ffmpeg"
link_media_tool ffprobe "utils/ffprobe"

if [[ "$DIM_SKIP_UI" == false ]]; then
    echo "Installing locked Eclipse dependencies..."
    corepack pnpm --dir eclipse install --frozen-lockfile
    corepack pnpm --dir eclipse build
fi

if [[ "$DIM_SKIP_RUST" == false ]]; then
    DIM_CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$DIM_ROOT_DIR/target"}
    if [[ "$DIM_CARGO_TARGET_DIR" != /* ]]; then
        DIM_CARGO_TARGET_DIR="$DIM_ROOT_DIR/$DIM_CARGO_TARGET_DIR"
    fi
    export CARGO_TARGET_DIR="$DIM_CARGO_TARGET_DIR"

    DIM_CARGO_ARGS=(build --locked)
    DIM_BINARY_PATH="$CARGO_TARGET_DIR/debug/dim"
    DIM_RUN_SUFFIX=""
    if [[ "$ECLIPSE_RELEASE_BUILD" == true ]]; then
        DIM_CARGO_ARGS+=(--release)
        DIM_BINARY_PATH="$CARGO_TARGET_DIR/release/dim"
        DIM_RUN_SUFFIX=" --release"
    fi

    echo "Building Dim..."
    cargo "${DIM_CARGO_ARGS[@]}"

    if [[ "$ECLIPSE_RELEASE_BUILD" == true ]]; then
        mkdir -p "$CARGO_TARGET_DIR/release/utils"
        link_media_tool ffmpeg "$CARGO_TARGET_DIR/release/utils/ffmpeg"
        link_media_tool ffprobe "$CARGO_TARGET_DIR/release/utils/ffprobe"
    fi

    echo "Dim is ready at $DIM_BINARY_PATH"
    echo "Run it with pnpm dev$DIM_RUN_SUFFIX"
fi
