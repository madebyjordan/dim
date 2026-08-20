#!/usr/bin/env bash

set -euo pipefail

ECLIPSE_ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
ECLIPSE_RELEASE_BUILD=false
ECLIPSE_SKIP_UI=false
ECLIPSE_SKIP_RUST=false
ECLIPSE_WINDOWS=false
ECLIPSE_STAGE_FILE=""

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) ECLIPSE_WINDOWS=true ;;
esac

usage() {
    echo "Usage: ./scripts/bootstrap.sh [--release] [--skip-ui] [--skip-rust] [--stage-file PATH]"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --release)
            ECLIPSE_RELEASE_BUILD=true
            ;;
        --skip-ui)
            ECLIPSE_SKIP_UI=true
            ;;
        --skip-rust)
            ECLIPSE_SKIP_RUST=true
            ;;
        --stage-file)
            [[ $# -ge 2 ]] || { echo "--stage-file requires a path." >&2; exit 2; }
            ECLIPSE_STAGE_FILE=$2
            shift
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

set_stage() {
    [[ -n "$ECLIPSE_STAGE_FILE" ]] || return 0
    printf '%s\n' "$1" > "$ECLIPSE_STAGE_FILE"
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Missing required command: $1" >&2
        exit 1
    fi
}

require_command git
require_command ffmpeg
require_command ffprobe

if [[ "$ECLIPSE_SKIP_UI" == false ]]; then
    require_command node
    require_command corepack

    if ! node -e '
        const [major, minor, patch] = process.versions.node.split(".").map(Number);
        process.exit(major === 24 && (minor > 19 || (minor === 19 && patch >= 0)) ? 0 : 1);
    '; then
        echo "Eclipse requires Node.js 24.19.0 or newer in the 24.x line; found $(node --version)." >&2
        exit 1
    fi
fi

if [[ "$ECLIPSE_SKIP_RUST" == false ]]; then
    require_command cargo
    require_command rustc
fi

cd "$ECLIPSE_ROOT_DIR"
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
    elif [[ "$ECLIPSE_WINDOWS" == true ]]; then
        cp "$source_path" "$destination"
        chmod +x "$destination"
    else
        ln -s "$source_path" "$destination"
    fi
}

ECLIPSE_MEDIA_SUFFIX=""
ECLIPSE_BINARY_SUFFIX=""
if [[ "$ECLIPSE_WINDOWS" == true ]]; then
    ECLIPSE_MEDIA_SUFFIX=".exe"
    ECLIPSE_BINARY_SUFFIX=".exe"
fi

link_media_tool ffmpeg "utils/ffmpeg$ECLIPSE_MEDIA_SUFFIX"
link_media_tool ffprobe "utils/ffprobe$ECLIPSE_MEDIA_SUFFIX"

if [[ "$ECLIPSE_SKIP_UI" == false ]]; then
    set_stage "Installing frontend dependencies"
    echo "Installing locked Eclipse dependencies..."
    corepack pnpm --dir eclipse install --frozen-lockfile
    set_stage "Building frontend"
    corepack pnpm --dir eclipse build
fi

if [[ "$ECLIPSE_SKIP_RUST" == false ]]; then
    ECLIPSE_CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$ECLIPSE_ROOT_DIR/target"}
    if [[ "$ECLIPSE_CARGO_TARGET_DIR" != /* ]]; then
        ECLIPSE_CARGO_TARGET_DIR="$ECLIPSE_ROOT_DIR/$ECLIPSE_CARGO_TARGET_DIR"
    fi
    export CARGO_TARGET_DIR="$ECLIPSE_CARGO_TARGET_DIR"

    ECLIPSE_CARGO_ARGS=(build --locked)
    ECLIPSE_BINARY_PATH="$CARGO_TARGET_DIR/debug/eclipse$ECLIPSE_BINARY_SUFFIX"
    ECLIPSE_RUN_SUFFIX=""
    if [[ "$ECLIPSE_RELEASE_BUILD" == true ]]; then
        ECLIPSE_CARGO_ARGS+=(--release)
        ECLIPSE_BINARY_PATH="$CARGO_TARGET_DIR/release/eclipse$ECLIPSE_BINARY_SUFFIX"
        ECLIPSE_RUN_SUFFIX=" --release"
    fi

    set_stage "Building Eclipse backend"
    echo "Building Eclipse..."
    cargo "${ECLIPSE_CARGO_ARGS[@]}"

    set_stage "Preparing runtime"
    if [[ "$ECLIPSE_RELEASE_BUILD" == true ]]; then
        mkdir -p "$CARGO_TARGET_DIR/release/utils"
        link_media_tool ffmpeg "$CARGO_TARGET_DIR/release/utils/ffmpeg$ECLIPSE_MEDIA_SUFFIX"
        link_media_tool ffprobe "$CARGO_TARGET_DIR/release/utils/ffprobe$ECLIPSE_MEDIA_SUFFIX"
    fi

    echo "Eclipse is ready at $ECLIPSE_BINARY_PATH"
    echo "Run it with corepack pnpm dev$ECLIPSE_RUN_SUFFIX"
fi

set_stage "Complete"
