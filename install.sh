#!/usr/bin/env bash

set -euo pipefail

ECLIPSE_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
if [[ -n "${ECLIPSE_URL+x}" ]]; then
    ECLIPSE_URL_OVERRIDDEN=true
else
    ECLIPSE_URL_OVERRIDDEN=false
fi
ECLIPSE_URL=${ECLIPSE_URL:-http://localhost:8000}
ECLIPSE_SELECTED_PLATFORM=""
ECLIPSE_AUTO_CONFIRM=false
ECLIPSE_START=true
ECLIPSE_DEMO=false
ECLIPSE_DEMO_REQUIREMENTS_RESOLVED=false
ECLIPSE_LOG=""
ECLIPSE_STEP_PID=""

if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
    ECLIPSE_BOLD=$'\033[1m'
    ECLIPSE_DIM=$'\033[2m'
    ECLIPSE_BLUE=$'\033[34m'
    ECLIPSE_GREEN=$'\033[32m'
    ECLIPSE_YELLOW=$'\033[33m'
    ECLIPSE_RED=$'\033[31m'
    ECLIPSE_RESET=$'\033[0m'
else
    ECLIPSE_BOLD=""
    ECLIPSE_DIM=""
    ECLIPSE_BLUE=""
    ECLIPSE_GREEN=""
    ECLIPSE_YELLOW=""
    ECLIPSE_RED=""
    ECLIPSE_RESET=""
fi

usage() {
    cat <<'EOF'
Usage: ./install.sh [--demo] [--platform macos|linux|windows] [--yes] [--no-start]

Without --platform, Eclipse Setup starts with an interactive platform selector.
Demo mode runs the same flow with deterministic fake checks and no system changes.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --platform)
            [[ $# -ge 2 ]] || { echo "--platform requires macos, linux, or windows." >&2; exit 2; }
            ECLIPSE_SELECTED_PLATFORM=$2
            shift
            ;;
        --yes)
            ECLIPSE_AUTO_CONFIRM=true
            ;;
        --demo)
            ECLIPSE_DEMO=true
            ;;
        --no-start)
            ECLIPSE_START=false
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

success() { printf '%s✓%s %s\n' "$ECLIPSE_GREEN" "$ECLIPSE_RESET" "$*"; }
notice() { printf '%s›%s %s\n' "$ECLIPSE_BLUE" "$ECLIPSE_RESET" "$*"; }
warning() { printf '%s!%s %s\n' "$ECLIPSE_YELLOW" "$ECLIPSE_RESET" "$*"; }
failure() { printf '%s✗%s %s\n' "$ECLIPSE_RED" "$ECLIPSE_RESET" "$*" >&2; }

cancel_install() {
    printf '\n' >&2
    if [[ -n "$ECLIPSE_STEP_PID" ]] && kill -0 "$ECLIPSE_STEP_PID" 2>/dev/null; then
        kill "$ECLIPSE_STEP_PID" 2>/dev/null || true
        wait "$ECLIPSE_STEP_PID" 2>/dev/null || true
    fi
    failure "Setup cancelled. Run $ECLIPSE_ROOT/install.sh whenever you are ready to continue."
    exit 130
}

trap cancel_install INT TERM

confirm() {
    local prompt=$1
    local reply
    if [[ "$ECLIPSE_AUTO_CONFIRM" == true ]]; then
        printf '%s?%s %s %sYes%s\n' "$ECLIPSE_BLUE" "$ECLIPSE_RESET" "$prompt" "$ECLIPSE_DIM" "$ECLIPSE_RESET"
        return 0
    fi
    printf '%s?%s %s %s[Y/n]%s ' "$ECLIPSE_BLUE" "$ECLIPSE_RESET" "$prompt" "$ECLIPSE_DIM" "$ECLIPSE_RESET"
    read -r reply
    [[ -z "$reply" || "$reply" == "y" || "$reply" == "Y" || "$reply" == "yes" || "$reply" == "Yes" ]]
}

select_platform() {
    local selected=0
    local key
    local escape
    local options=("macOS" "Linux" "Windows")

    if [[ ! -t 0 || ! -t 1 ]]; then
        failure "Interactive setup needs a terminal. Re-run ./install.sh, or pass --platform explicitly."
        exit 2
    fi

    printf '%sWhich platform are you installing on?%s\n\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"
    while true; do
        local index=0
        while [[ $index -lt ${#options[@]} ]]; do
            if [[ $index -eq $selected ]]; then
                printf '%s❯ %s%s\n' "$ECLIPSE_BLUE" "${options[$index]}" "$ECLIPSE_RESET"
            else
                printf '  %s\n' "${options[$index]}"
            fi
            index=$((index + 1))
        done

        IFS= read -rsn1 key
        if [[ "$key" == $'\033' ]]; then
            IFS= read -rsn1 escape || true
            IFS= read -rsn1 escape || true
            if [[ "$escape" == "A" && $selected -gt 0 ]]; then
                selected=$((selected - 1))
            elif [[ "$escape" == "B" && $selected -lt $((${#options[@]} - 1)) ]]; then
                selected=$((selected + 1))
            fi
        elif [[ -z "$key" ]]; then
            break
        elif [[ "$key" == "k" && $selected -gt 0 ]]; then
            selected=$((selected - 1))
        elif [[ "$key" == "j" && $selected -lt $((${#options[@]} - 1)) ]]; then
            selected=$((selected + 1))
        fi
        printf '\033[%sA' "${#options[@]}"
    done

    case $selected in
        0) ECLIPSE_SELECTED_PLATFORM=macos ;;
        1) ECLIPSE_SELECTED_PLATFORM=linux ;;
        2) ECLIPSE_SELECTED_PLATFORM=windows ;;
    esac
    printf '\n'
}

run_step() {
    local label=$1
    shift
    local log
    local pid
    local frame=0
    local frames=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')

    if [[ "$ECLIPSE_DEMO" == true ]]; then
        if [[ -t 1 ]]; then
            while [[ $frame -lt 8 ]]; do
                printf '\r%s%s%s %s' "$ECLIPSE_BLUE" "${frames[$frame]}" "$ECLIPSE_RESET" "$label"
                sleep 0.08
                frame=$((frame + 1))
            done
            printf '\r\033[2K'
        fi
        success "$label"
        return 0
    fi

    log=$(mktemp "${TMPDIR:-/tmp}/eclipse-install.XXXXXX")
    ECLIPSE_LOG=$log

    "$@" >"$log" 2>&1 &
    pid=$!
    ECLIPSE_STEP_PID=$pid
    if [[ -t 1 ]]; then
        while kill -0 "$pid" 2>/dev/null; do
            printf '\r%s%s%s %s' "$ECLIPSE_BLUE" "${frames[$frame]}" "$ECLIPSE_RESET" "$label"
            frame=$(((frame + 1) % ${#frames[@]}))
            sleep 0.1
        done
        printf '\r\033[2K'
    fi

    if wait "$pid"; then
        ECLIPSE_STEP_PID=""
        success "$label"
        rm -f "$log"
        ECLIPSE_LOG=""
        return 0
    fi

    ECLIPSE_STEP_PID=""
    failure "$label failed."
    if [[ -s "$log" ]]; then
        printf '\n%sDiagnostic output:%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET" >&2
        tail -n 30 "$log" >&2
    fi
    printf '\n%sFull log:%s %s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET" "$log" >&2
    return 1
}

command_available() {
    command -v "$1" >/dev/null 2>&1
}

node_is_supported() {
    command_available node && node -e '
        const [major, minor, patch] = process.versions.node.split(".").map(Number);
        process.exit(major === 24 && (minor > 19 || (minor === 19 && patch >= 0)) ? 0 : 1);
    ' >/dev/null 2>&1
}

media_tool_is_supported() {
    local tool=$1
    command_available "$tool" && "$tool" -version 2>/dev/null | head -n 1 | awk '
        { for (i = 1; i <= NF; i++) if ($i ~ /^[0-9]+\./) { found = 1; split($i, version, "."); exit(version[1] >= 6 ? 0 : 1) } }
        END { if (!found) exit 1 }
    '
}

check_existing_media_tools() {
    local path
    for path in "$ECLIPSE_ROOT/utils/ffmpeg" "$ECLIPSE_ROOT/utils/ffprobe" \
        "$ECLIPSE_ROOT/target/release/utils/ffmpeg" "$ECLIPSE_ROOT/target/release/utils/ffprobe"; do
        if [[ -e "$path" || -L "$path" ]]; then
            if [[ ! -x "$path" ]]; then
                failure "$path already exists but is not executable. Eclipse will not replace it."
                printf 'Repair or remove that specific file, then run %s again.\n' "$ECLIPSE_ROOT/install.sh" >&2
                return 1
            fi
        fi
    done
}

print_requirement_failure() {
    local item=$1
    case "$item" in
        xcode) printf '  • Apple Command Line Tools — run: xcode-select --install\n' ;;
        git) printf '  • Git — supplied by Apple Command Line Tools\n' ;;
        node) printf '  • Node.js 24.19.0 or newer in the 24.x line — Homebrew package: node@24\n' ;;
        corepack) printf '  • Corepack — included with supported Node.js; then run: corepack enable pnpm\n' ;;
        rustup) printf '  • Rustup and the repository-pinned Rust 1.93.1 toolchain — https://rustup.rs\n' ;;
        ffmpeg) printf '  • FFmpeg and FFprobe 6.0 or newer — Homebrew package: ffmpeg\n' ;;
        sqlite) printf '  • SQLite tools — Homebrew package: sqlite\n' ;;
        pkgconfig) printf '  • pkg-config — Homebrew package: pkg-config\n' ;;
        curl) printf '  • curl — supplied by macOS and needed to install Rustup\n' ;;
    esac
}

collect_macos_requirements() {
    ECLIPSE_MISSING=()
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        if [[ "$ECLIPSE_DEMO_REQUIREMENTS_RESOLVED" == false ]]; then
            ECLIPSE_MISSING=(ffmpeg pkgconfig)
        fi
        return 0
    fi
    xcode-select -p >/dev/null 2>&1 || ECLIPSE_MISSING+=(xcode)
    command_available git || ECLIPSE_MISSING+=(git)
    node_is_supported || ECLIPSE_MISSING+=(node)
    command_available corepack || ECLIPSE_MISSING+=(corepack)
    if ! command_available rustup || ! command_available cargo || ! command_available rustc; then ECLIPSE_MISSING+=(rustup); fi
    if ! media_tool_is_supported ffmpeg || ! media_tool_is_supported ffprobe; then ECLIPSE_MISSING+=(ffmpeg); fi
    command_available sqlite3 || ECLIPSE_MISSING+=(sqlite)
    command_available pkg-config || ECLIPSE_MISSING+=(pkgconfig)
    command_available curl || ECLIPSE_MISSING+=(curl)
}

prepare_macos_path() {
    [[ "$ECLIPSE_DEMO" == false ]] || return 0
    if [[ -d "${CARGO_HOME:-$HOME/.cargo}/bin" ]]; then
        export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
    fi
    if command_available brew; then
        if brew --prefix node@24 >/dev/null 2>&1; then
            export PATH="$(brew --prefix node@24)/bin:$PATH"
        fi
        if brew --prefix sqlite >/dev/null 2>&1; then
            export PATH="$(brew --prefix sqlite)/bin:$PATH"
        fi
    fi
}

install_homebrew_requirements() {
    local packages=()
    local item
    for item in "${ECLIPSE_MISSING[@]}"; do
        case "$item" in
            node|corepack) [[ " ${packages[*]} " == *" node@24 "* ]] || packages+=(node@24) ;;
            ffmpeg) packages+=(ffmpeg) ;;
            sqlite) packages+=(sqlite) ;;
            pkgconfig) packages+=(pkg-config) ;;
        esac
    done
    if [[ ${#packages[@]} -gt 0 ]]; then
        if [[ "$ECLIPSE_DEMO" == false ]]; then
            command_available brew || return 1
        fi
        confirm "Install missing Homebrew packages (${packages[*]})?" || return 1
        run_step "Installed Homebrew requirements" brew install "${packages[@]}" || return 1
        if [[ "$ECLIPSE_DEMO" == true ]]; then
            ECLIPSE_DEMO_REQUIREMENTS_RESOLVED=true
            return 0
        fi
        if brew --prefix node@24 >/dev/null 2>&1; then
            export PATH="$(brew --prefix node@24)/bin:$PATH"
        fi
        if brew --prefix sqlite >/dev/null 2>&1; then
            export PATH="$(brew --prefix sqlite)/bin:$PATH"
        fi
    fi
}

install_rustup() {
    local needs_rustup=false
    local item
    for item in "${ECLIPSE_MISSING[@]}"; do
        [[ "$item" == rustup ]] && needs_rustup=true
    done
    [[ "$needs_rustup" == true ]] || return 0
    command_available curl || return 1
    confirm "Install Rustup from rustup.rs?" || return 1
    run_step "Installed Rustup" sh -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal' || return 1
    export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
}

resolve_macos_requirements() {
    local has_xcode=false
    local item
    local brew_needed=false
    for item in "${ECLIPSE_MISSING[@]}"; do
        [[ "$item" == xcode ]] && has_xcode=true
        case "$item" in node|corepack|ffmpeg|sqlite|pkgconfig) brew_needed=true ;; esac
    done

    if [[ "$has_xcode" == true ]]; then
        printf '\n'
        warning "Apple Command Line Tools are required before Eclipse can be built."
        if confirm "Open the Apple Command Line Tools installer now?"; then
            xcode-select --install >/dev/null 2>&1 || true
            printf 'Complete Apple’s installer, then run:\n  %s\n' "$ECLIPSE_ROOT/install.sh"
        else
            printf 'Run this when ready, then launch Eclipse Setup again:\n  xcode-select --install\n'
        fi
        return 1
    fi

    if [[ "$ECLIPSE_DEMO" == false && "$brew_needed" == true ]] && ! command_available brew; then
        printf '\n'
        failure "Homebrew is not installed, and macOS packages are missing."
        printf 'Install Homebrew from https://brew.sh, then run:\n  %s\n\nMissing requirements:\n' "$ECLIPSE_ROOT/install.sh"
        for item in "${ECLIPSE_MISSING[@]}"; do print_requirement_failure "$item"; done
        return 1
    fi

    install_homebrew_requirements || true
    install_rustup || true
    collect_macos_requirements
    if [[ ${#ECLIPSE_MISSING[@]} -gt 0 ]]; then
        printf '\n'
        failure "Some requirements still need attention:"
        for item in "${ECLIPSE_MISSING[@]}"; do print_requirement_failure "$item"; done
        printf '\nAfter resolving them, run:\n  %s\n' "$ECLIPSE_ROOT/install.sh"
        return 1
    fi
}

check_macos_requirements() {
    printf '%sChecking requirements%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"
    collect_macos_requirements
    if [[ ${#ECLIPSE_MISSING[@]} -gt 0 ]]; then
        warning "Found ${#ECLIPSE_MISSING[@]} missing or unsupported requirement(s)."
        resolve_macos_requirements || return 1
    fi
    run_step "Prepared pinned Rust 1.93.1 toolchain" rustup toolchain install 1.93.1 --profile minimal --component rustfmt --component clippy
    success "Git, Node 24, Corepack, Rust, FFmpeg/FFprobe, SQLite, pkg-config, and build tools are ready"
}

wait_for_eclipse() {
    local pid=$1
    local attempts=0
    while [[ $attempts -lt 60 ]]; do
        if curl --fail --silent "$ECLIPSE_URL/health/ready" >/dev/null 2>&1; then
            return 0
        fi
        kill -0 "$pid" 2>/dev/null || return 1
        attempts=$((attempts + 1))
        sleep 1
    done
    return 1
}

use_configured_local_url() {
    local config="$ECLIPSE_ROOT/target/release/config/config.toml"
    local port
    [[ "$ECLIPSE_DEMO" == false && "$ECLIPSE_URL_OVERRIDDEN" == false && -f "$config" ]] || return 0
    port=$(awk -F= '
        /^[[:space:]]*port[[:space:]]*=/ {
            value = $2
            sub(/#.*/, "", value)
            gsub(/[[:space:]]/, "", value)
            if (value ~ /^[0-9]+$/) print value
            exit
        }
    ' "$config")
    if [[ -n "$port" ]]; then
        ECLIPSE_URL="http://localhost:$port"
    fi
}

start_eclipse() {
    local runtime_dir="$ECLIPSE_ROOT/target/release"
    local log="$runtime_dir/eclipse.log"
    local pid

    if [[ "$ECLIPSE_DEMO" == true ]]; then
        notice "Starting Eclipse in the background"
        run_step "Eclipse is running" true
        printf '\n%sEclipse is ready:%s %s%s%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET" "$ECLIPSE_BLUE" "$ECLIPSE_URL" "$ECLIPSE_RESET"
        if confirm "Open Eclipse in your default browser?"; then
            run_step "Opened Eclipse" true
        fi
        return 0
    fi

    if curl --fail --silent "$ECLIPSE_URL/health/ready" >/dev/null 2>&1; then
        success "Eclipse is already running"
    else
        notice "Starting Eclipse in the background"
        nohup "$ECLIPSE_ROOT/scripts/run.sh" --release >"$log" 2>&1 &
        pid=$!
        printf '%s\n' "$pid" > "$runtime_dir/eclipse.pid"
        if ! wait_for_eclipse "$pid"; then
            failure "Eclipse did not become ready at $ECLIPSE_URL."
            if [[ -s "$log" ]]; then
                printf '\n%sRecent Eclipse output:%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET" >&2
                tail -n 30 "$log" >&2
            fi
            printf '\nFix the reported problem, then run:\n  %s --release\n' "$ECLIPSE_ROOT/scripts/run.sh" >&2
            return 1
        fi
        success "Eclipse is running"
    fi

    printf '\n%sEclipse is ready:%s %s%s%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET" "$ECLIPSE_BLUE" "$ECLIPSE_URL" "$ECLIPSE_RESET"
    if confirm "Open Eclipse in your default browser?"; then
        if open "$ECLIPSE_URL"; then
            success "Opened Eclipse"
        else
            warning "The browser could not be opened automatically. Open $ECLIPSE_URL yourself."
        fi
    fi
}

install_platform_macos() {
    if [[ "$ECLIPSE_DEMO" == false && $(uname -s) != Darwin ]]; then
        failure "The macOS installer must be run on macOS. You selected macOS, but this system reports $(uname -s)."
        return 1
    fi

    notice "macOS selected"
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        notice "Demo mode — all checks and actions are simulated"
    fi
    prepare_macos_path
    if [[ "$ECLIPSE_DEMO" == false ]]; then
        check_existing_media_tools
    fi
    check_macos_requirements
    printf '\n%sInstalling Eclipse%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"
    if [[ "$ECLIPSE_DEMO" == true || -f "$ECLIPSE_ROOT/target/release/config/config.toml" ]]; then
        notice "Existing configuration found; it will be preserved"
    fi
    run_step "Built Eclipse with locked dependencies" "$ECLIPSE_ROOT/scripts/bootstrap.sh" --release
    success "Existing configuration and media-tool links were preserved"
    use_configured_local_url

    if [[ "$ECLIPSE_START" == true ]] && confirm "Start Eclipse now?"; then
        start_eclipse
    else
        printf '\n%sSetup complete.%s Start Eclipse with:\n  %s --release\n\nThen open %s\n' \
            "$ECLIPSE_BOLD" "$ECLIPSE_RESET" "$ECLIPSE_ROOT/scripts/run.sh" "$ECLIPSE_URL"
    fi
}

install_platform_linux() {
    notice "Linux selected"
    warning "The interactive Linux installer is not available in this first release."
    printf 'Linux source development remains supported with the existing documented workflow:\n  pnpm build --release\n  pnpm dev --release\n\nNo changes were made.\n'
}

install_platform_windows() {
    notice "Windows selected"
    warning "Windows installation is not supported by this first installer release."
    printf 'Use macOS for the guided setup today. No changes were made.\n'
}

printf '\n%sEclipse Setup%s\n\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"

if [[ -z "$ECLIPSE_SELECTED_PLATFORM" ]]; then
    select_platform
fi

case "$ECLIPSE_SELECTED_PLATFORM" in
    macos) install_platform_macos ;;
    linux) install_platform_linux ;;
    windows) install_platform_windows ;;
    *)
        failure "Unknown platform '$ECLIPSE_SELECTED_PLATFORM'. Choose macos, linux, or windows."
        exit 2
        ;;
esac
