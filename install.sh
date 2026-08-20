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
ECLIPSE_DEMO_SCENARIO=fresh
ECLIPSE_DEMO_REQUIREMENTS_RESOLVED=false
ECLIPSE_MENU_SELECTION=0
ECLIPSE_NEXT_MENU_SELECTION=0
ECLIPSE_LOG=""
ECLIPSE_STEP_PID=""
ECLIPSE_WINDOWS_TOOLCHAIN_DETAIL=""
ECLIPSE_WINDOWS_TOOLCHAIN_STATUS="toolchain-inconclusive"
ECLIPSE_INSTALL_MODE=fresh
ECLIPSE_EXISTING_ACTION=""
ECLIPSE_EXISTING_INSTALLATION=false
ECLIPSE_RUNNING_PIDS=()
ECLIPSE_MANAGED_PATHS=()
ECLIPSE_LIFECYCLE_LOG=""
ECLIPSE_LIFECYCLE_DIAGNOSTICS_SHOWN=false
ECLIPSE_SHUTDOWN_ATTEMPTED=false
ECLIPSE_RUNTIME_DIR="$ECLIPSE_ROOT/target/release"
ECLIPSE_BINARY_SUFFIX=""
ECLIPSE_MEDIA_STATUS="unknown"
ECLIPSE_FFMPEG_STATUS="unknown"
ECLIPSE_FFPROBE_STATUS="unknown"
ECLIPSE_FFMPEG_MAJOR=""
ECLIPSE_FFPROBE_MAJOR=""

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
Usage: ./install.sh [--demo] [--demo-scenario SCENARIO] [--existing-action ACTION] [--platform macos|linux|windows] [--yes] [--no-start]
Windows CMD or PowerShell: install.cmd [--demo] [--demo-scenario SCENARIO] [--existing-action ACTION] [--platform windows] [--yes] [--no-start]

Without --platform, Eclipse Setup starts with an interactive platform selector.
Demo mode runs the same flow with deterministic fake checks and no system changes.
Demo scenarios: fresh, reinstall, reset, clean, or exit.
Existing-install actions for automation: reinstall, reset, clean, or exit (requires --yes).
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
        --demo-scenario)
            [[ $# -ge 2 ]] || { echo "--demo-scenario requires fresh, reinstall, reset, clean, or exit." >&2; exit 2; }
            ECLIPSE_DEMO_SCENARIO=$2
            shift
            ;;
        --existing-action)
            [[ $# -ge 2 ]] || { echo "--existing-action requires reinstall, reset, clean, or exit." >&2; exit 2; }
            ECLIPSE_EXISTING_ACTION=$2
            shift
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

case "$ECLIPSE_DEMO_SCENARIO" in
    fresh|reinstall|reset|clean|exit) ;;
    *) echo "Unknown demo scenario: $ECLIPSE_DEMO_SCENARIO" >&2; exit 2 ;;
esac
case "$ECLIPSE_EXISTING_ACTION" in
    ""|reinstall|reset|clean|exit) ;;
    *) echo "Unknown existing-install action: $ECLIPSE_EXISTING_ACTION" >&2; exit 2 ;;
esac
if [[ -n "$ECLIPSE_EXISTING_ACTION" && "$ECLIPSE_AUTO_CONFIRM" != true ]]; then
    echo "--existing-action requires --yes so confirmations remain explicit." >&2
    exit 2
fi

success() { printf '%s✓%s %s\n' "$ECLIPSE_GREEN" "$ECLIPSE_RESET" "$*"; }
notice() { printf '%s›%s %s\n' "$ECLIPSE_BLUE" "$ECLIPSE_RESET" "$*"; }
warning() { printf '%s!%s %s\n' "$ECLIPSE_YELLOW" "$ECLIPSE_RESET" "$*"; }
failure() { printf '%s✗%s %s\n' "$ECLIPSE_RED" "$ECLIPSE_RESET" "$*" >&2; }

setup_command() {
    if [[ "$ECLIPSE_SELECTED_PLATFORM" == windows ]]; then
        printf 'install.cmd'
    else
        printf '%s/install.sh' "$ECLIPSE_ROOT"
    fi
}

cancel_install() {
    printf '\n' >&2
    if [[ -n "$ECLIPSE_STEP_PID" ]] && kill -0 "$ECLIPSE_STEP_PID" 2>/dev/null; then
        kill "$ECLIPSE_STEP_PID" 2>/dev/null || true
        wait "$ECLIPSE_STEP_PID" 2>/dev/null || true
    fi
    failure "Setup cancelled. Run $(setup_command) whenever you are ready to continue."
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

select_menu() {
    local prompt=$1
    local allow_auto=$2
    shift 2
    local selected=$ECLIPSE_NEXT_MENU_SELECTION
    local key
    local escape
    local options=("$@")
    ECLIPSE_NEXT_MENU_SELECTION=0

    if [[ -n "$prompt" ]]; then
        printf '%s%s%s\n\n' "$ECLIPSE_BOLD" "$prompt" "$ECLIPSE_RESET"
    fi

    while true; do
        local index=0
        while [[ $index -lt ${#options[@]} ]]; do
            if [[ $index -eq $selected ]]; then
                printf '▶ %s\n' "${options[$index]}"
            else
                printf '  %s\n' "${options[$index]}"
            fi
            index=$((index + 1))
        done

        if [[ "$allow_auto" == true && "$ECLIPSE_AUTO_CONFIRM" == true ]]; then
            break
        fi
        if [[ ! -t 0 || ! -t 1 ]]; then
            failure "Interactive setup needs a terminal. Re-run ./install.sh in a terminal."
            exit 2
        fi

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

    ECLIPSE_MENU_SELECTION=$selected
    printf '\n'
}

select_platform() {
    select_menu "Which platform are you installing on?" false "macOS" "Linux" "Windows"

    case $ECLIPSE_MENU_SELECTION in
        0) ECLIPSE_SELECTED_PLATFORM=macos ;;
        1) ECLIPSE_SELECTED_PLATFORM=linux ;;
        2) ECLIPSE_SELECTED_PLATFORM=windows ;;
    esac
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

add_running_pid() {
    local candidate=$1
    local existing
    candidate=${candidate//$'\r'/}
    [[ "$candidate" =~ ^[0-9]+$ ]] || return 0
    for existing in "${ECLIPSE_RUNNING_PIDS[@]:-}"; do
        [[ "$existing" == "$candidate" ]] && return 0
    done
    ECLIPSE_RUNNING_PIDS+=("$candidate")
}

ensure_lifecycle_log() {
    [[ "$ECLIPSE_DEMO" == false ]] || return 0
    [[ -z "$ECLIPSE_LIFECYCLE_LOG" ]] || return 0
    mkdir -p "$ECLIPSE_RUNTIME_DIR/logs"
    ECLIPSE_LIFECYCLE_LOG=$(mktemp "$ECLIPSE_RUNTIME_DIR/logs/install-lifecycle.XXXXXX.log")
}

lifecycle_debug() {
    [[ -n "$ECLIPSE_LIFECYCLE_LOG" ]] || return 0
    printf '%s\n' "$*" >> "$ECLIPSE_LIFECYCLE_LOG"
}

surface_lifecycle_diagnostics() {
    [[ -n "$ECLIPSE_LIFECYCLE_LOG" && -s "$ECLIPSE_LIFECYCLE_LOG" ]] || return 0
    [[ "$ECLIPSE_LIFECYCLE_DIAGNOSTICS_SHOWN" == false ]] || return 0
    ECLIPSE_LIFECYCLE_DIAGNOSTICS_SHOWN=true
    printf '\n%sProcess lifecycle diagnostic:%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET" >&2
    tail -n 40 "$ECLIPSE_LIFECYCLE_LOG" >&2
    printf '%sFull lifecycle log:%s %s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET" "$ECLIPSE_LIFECYCLE_LOG" >&2
}

canonical_existing_file() {
    local path=$1
    [[ -e "$path" ]] || return 1
    (cd "$(dirname "$path")" && printf '%s/%s\n' "$PWD" "$(basename "$path")")
}

process_matches_unix_runtime() {
    local pid=$1
    local eclipse_binary=$2
    local legacy_binary=$3
    local executable=""
    local command_line=""

    if [[ -e "/proc/$pid/exe" ]]; then
        executable=$(readlink "/proc/$pid/exe" 2>/dev/null || true)
        executable=${executable% (deleted)}
        [[ "$executable" == "$eclipse_binary" || "$executable" == "$legacy_binary" ]]
        return
    fi

    command_line=$(ps -p "$pid" -o command= 2>/dev/null || true)
    [[ "$command_line" == "$eclipse_binary" || "$command_line" == "$eclipse_binary "* ||
       "$command_line" == "$legacy_binary" || "$command_line" == "$legacy_binary "* ]]
}

detect_windows_runtime_processes() {
    local eclipse_binary=$1
    local legacy_binary=$2
    local targets
    local detector_error_log=${ECLIPSE_LIFECYCLE_LOG:-/dev/null}
    local recorded_pid=""
    local pid

    if command_available cygpath; then
        eclipse_binary=$(cygpath -w "$eclipse_binary")
        [[ -z "$legacy_binary" ]] || legacy_binary=$(cygpath -w "$legacy_binary")
    fi
    targets="$eclipse_binary"
    [[ -z "$legacy_binary" ]] || targets="$targets;$legacy_binary"
    if [[ -f "$ECLIPSE_RUNTIME_DIR/eclipse.pid" ]]; then
        recorded_pid=$(tr -dc '0-9' < "$ECLIPSE_RUNTIME_DIR/eclipse.pid")
    fi
    lifecycle_debug "detect targets=$targets recorded_pid=${recorded_pid:-none}"
    while IFS= read -r pid; do
        add_running_pid "$pid"
    done < <(
        ECLIPSE_PROCESS_TARGETS="$targets" \
        ECLIPSE_RECORDED_PID="$recorded_pid" \
        powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '
            $comparison = [StringComparison]::OrdinalIgnoreCase
            $targets = @($env:ECLIPSE_PROCESS_TARGETS.Split(";") |
                Where-Object { $_ } |
                ForEach-Object { [IO.Path]::GetFullPath($_) })
            $recordedPid = 0
            [void][int]::TryParse($env:ECLIPSE_RECORDED_PID, [ref]$recordedPid)

            function Add-LifecycleTrace([string] $message) {
                [Console]::Error.WriteLine($message)
            }

            function Find-CommandTarget([string] $commandLine) {
                if (-not $commandLine) { return $null }
                $commandLine = $commandLine.Trim()
                foreach ($target in $targets) {
                    if ($commandLine.Equals($target, $comparison) -or
                        $commandLine.StartsWith($target + " ", $comparison) -or
                        $commandLine.Equals("`"$target`"", $comparison) -or
                        $commandLine.StartsWith("`"$target`" ", $comparison)) {
                        return $target
                    }
                }
                return $null
            }

            Add-LifecycleTrace("windows process scan started targets=" + ($targets -join ";"))
            Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
                ForEach-Object {
                    $process = $_
                    $normalizedExecutable = $null
                    if ($process.ExecutablePath) {
                        try {
                            $normalizedExecutable = [IO.Path]::GetFullPath($process.ExecutablePath)
                        } catch {
                            $normalizedExecutable = $null
                        }
                    }
                    $executableTarget = $targets | Where-Object {
                        $normalizedExecutable -and $_.Equals($normalizedExecutable, $comparison)
                    } | Select-Object -First 1
                    $commandTarget = Find-CommandTarget $process.CommandLine
                    $namedCandidate = $process.Name -and
                        ($process.Name.Equals("eclipse.exe", $comparison) -or
                         $process.Name.Equals("dim.exe", $comparison))
                    $recordedCandidate = $recordedPid -gt 0 -and $process.ProcessId -eq $recordedPid
                    if (-not ($namedCandidate -or $recordedCandidate -or $executableTarget -or $commandTarget)) {
                        return
                    }

                    $accepted = [bool]($executableTarget -or $commandTarget)
                    $kind = if ($executableTarget) {
                        [IO.Path]::GetFileName($executableTarget)
                    } elseif ($commandTarget) {
                        [IO.Path]::GetFileName($commandTarget)
                    } else {
                        "another path"
                    }
                    $reason = if ($executableTarget) {
                        "exact normalized ExecutablePath"
                    } elseif ($commandTarget) {
                        "exact command-line executable"
                    } elseif ($recordedCandidate) {
                        "recorded PID lacks exact path evidence"
                    } else {
                        "process name only"
                    }
                    $executableDisplay = if ($process.ExecutablePath) {
                        $process.ExecutablePath
                    } else {
                        "<unavailable>"
                    }
                    $normalizedDisplay = if ($normalizedExecutable) {
                        $normalizedExecutable
                    } else {
                        "<unavailable>"
                    }
                    $commandDisplay = if ($process.CommandLine) {
                        $process.CommandLine
                    } else {
                        "<unavailable>"
                    }
                    Add-LifecycleTrace(
                        "candidate pid=$($process.ProcessId) name=$($process.Name) kind=$kind " +
                        "executable=$executableDisplay normalized=$normalizedDisplay " +
                        "accepted=$accepted reason=$reason command=$commandDisplay"
                    )
                    if ($accepted) {
                        [Console]::Out.Write("$($process.ProcessId)`n")
                    }
                }
        ' 2>>"$detector_error_log" || lifecycle_debug "windows process scan command failed"
    )
    lifecycle_debug "detect accepted_pids=${ECLIPSE_RUNNING_PIDS[*]:-none}"
}

detect_unix_runtime_processes() {
    local eclipse_binary=$1
    local legacy_binary=$2
    local pid=""
    local command_line=""

    if [[ -f "$ECLIPSE_RUNTIME_DIR/eclipse.pid" ]]; then
        pid=$(tr -dc '0-9' < "$ECLIPSE_RUNTIME_DIR/eclipse.pid")
        if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null &&
            process_matches_unix_runtime "$pid" "$eclipse_binary" "$legacy_binary"; then
            add_running_pid "$pid"
        fi
    fi

    while read -r pid command_line; do
        [[ -n "$pid" ]] || continue
        if [[ "$command_line" == "$eclipse_binary" || "$command_line" == "$eclipse_binary "* ||
              "$command_line" == "$legacy_binary" || "$command_line" == "$legacy_binary "* ]]; then
            add_running_pid "$pid"
        fi
    done < <(ps -ax -o pid= -o command= 2>/dev/null || true)
}

detect_running_eclipse() {
    local eclipse_binary="$ECLIPSE_RUNTIME_DIR/eclipse$ECLIPSE_BINARY_SUFFIX"
    local legacy_binary="$ECLIPSE_RUNTIME_DIR/dim$ECLIPSE_BINARY_SUFFIX"
    local canonical

    ECLIPSE_RUNNING_PIDS=()
    if [[ "$ECLIPSE_DEMO" == true && "$ECLIPSE_DEMO_SCENARIO" != fresh ]]; then
        ECLIPSE_RUNNING_PIDS=(4242)
        return 0
    fi
    [[ "$ECLIPSE_DEMO" == false ]] || return 1

    canonical=$(canonical_existing_file "$eclipse_binary" 2>/dev/null || true)
    [[ -n "$canonical" ]] && eclipse_binary=$canonical
    canonical=$(canonical_existing_file "$legacy_binary" 2>/dev/null || true)
    [[ -n "$canonical" ]] && legacy_binary=$canonical

    if [[ "$ECLIPSE_SELECTED_PLATFORM" == windows ]]; then
        detect_windows_runtime_processes "$eclipse_binary" "$legacy_binary"
    else
        detect_unix_runtime_processes "$eclipse_binary" "$legacy_binary"
    fi
    [[ ${#ECLIPSE_RUNNING_PIDS[@]} -gt 0 ]]
}

detect_existing_installation() {
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        [[ "$ECLIPSE_DEMO_SCENARIO" != fresh ]]
        return
    fi

    [[ -f "$ECLIPSE_RUNTIME_DIR/eclipse$ECLIPSE_BINARY_SUFFIX" ||
       -f "$ECLIPSE_RUNTIME_DIR/dim$ECLIPSE_BINARY_SUFFIX" ||
       -f "$ECLIPSE_RUNTIME_DIR/config/config.toml" ||
       -f "$ECLIPSE_RUNTIME_DIR/config/dim.db" ||
       -d "$ECLIPSE_RUNTIME_DIR/metadata" ||
       -d "$ECLIPSE_RUNTIME_DIR/streaming_cache" ]] && return 0

    detect_running_eclipse
}

exit_without_changes() {
    success "No changes made"
    exit 0
}

prepare_installation_lifecycle() {
    local selection=0

    ECLIPSE_BINARY_SUFFIX=""
    [[ "$ECLIPSE_SELECTED_PLATFORM" == windows ]] && ECLIPSE_BINARY_SUFFIX=".exe"
    detect_existing_installation || return 0
    ECLIPSE_EXISTING_INSTALLATION=true

    printf '%sExisting Eclipse installation detected.%s\n\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"
    if [[ -n "$ECLIPSE_EXISTING_ACTION" ]]; then
        case "$ECLIPSE_EXISTING_ACTION" in
            reinstall) ECLIPSE_NEXT_MENU_SELECTION=0 ;;
            reset) ECLIPSE_NEXT_MENU_SELECTION=1 ;;
            clean) ECLIPSE_NEXT_MENU_SELECTION=2 ;;
            exit) ECLIPSE_NEXT_MENU_SELECTION=3 ;;
        esac
    elif [[ "$ECLIPSE_DEMO" == true ]]; then
        case "$ECLIPSE_DEMO_SCENARIO" in
            reinstall) ECLIPSE_NEXT_MENU_SELECTION=0 ;;
            reset) ECLIPSE_NEXT_MENU_SELECTION=1 ;;
            clean) ECLIPSE_NEXT_MENU_SELECTION=2 ;;
            exit) ECLIPSE_NEXT_MENU_SELECTION=3 ;;
        esac
    fi
    select_menu "What would you like to do?" true \
        "Reinstall / update Eclipse" "Reset Eclipse" "Clean install" "Exit"
    selection=$ECLIPSE_MENU_SELECTION

    case $selection in
        0)
            ECLIPSE_INSTALL_MODE=reinstall
            printf 'Configuration, accounts, libraries, metadata, and other persistent state will be preserved.\n'
            ;;
        1)
            ECLIPSE_INSTALL_MODE=reset
            printf 'Reset removes host settings, streaming cache, and logs.\n'
            printf 'Accounts, libraries, indexed media state, watch progress, and metadata are preserved.\n'
            printf 'A new host secret is generated, so existing browser sessions must sign in again.\n'
            confirm "Continue with Reset Eclipse?" || exit_without_changes
            ;;
        2)
            ECLIPSE_INSTALL_MODE=clean
            printf 'Clean install permanently removes Eclipse-managed configuration, accounts, libraries,\n'
            printf 'indexed media state, watch progress, sessions, metadata, streaming cache, and logs.\n'
            printf 'Media source files and the source repository are not removed.\n'
            confirm "Permanently remove this Eclipse installation's managed data?" || exit_without_changes
            ;;
        3) exit_without_changes ;;
    esac

    if [[ "$ECLIPSE_INSTALL_MODE" == clean ]]; then
        ensure_lifecycle_log
        lifecycle_debug "clean install confirmed runtime=${ECLIPSE_RUNTIME_DIR} platform=${ECLIPSE_SELECTED_PLATFORM}"
    fi
    if detect_running_eclipse; then
        warning "Running Eclipse detected (PID(s): ${ECLIPSE_RUNNING_PIDS[*]})."
        printf 'Eclipse must be stopped before its runtime files can be changed.\n'
    fi
}

wait_for_processes_to_stop() {
    local attempts=0
    local pid
    lifecycle_debug "waiting for verified process exit pids=${ECLIPSE_RUNNING_PIDS[*]:-none} timeout_seconds=20"
    while [[ $attempts -lt 80 ]]; do
        if [[ "$ECLIPSE_SELECTED_PLATFORM" == windows ]]; then
            if ! detect_running_eclipse; then
                lifecycle_debug "verified process exited during graceful wait attempt=$attempts"
                return 0
            fi
            attempts=$((attempts + 1))
            sleep 0.25
            continue
        fi
        local alive=false
        for pid in "${ECLIPSE_RUNNING_PIDS[@]}"; do
            if kill -0 "$pid" 2>/dev/null; then
                alive=true
                break
            fi
        done
        if [[ "$alive" == false ]]; then
            lifecycle_debug "verified process exited during graceful wait attempt=$attempts"
            return 0
        fi
        attempts=$((attempts + 1))
        sleep 0.25
    done
    lifecycle_debug "verified process remained alive through graceful timeout pids=${ECLIPSE_RUNNING_PIDS[*]:-none}"
    return 1
}

format_elapsed() {
    local seconds=$1
    printf '%02d:%02d' "$((seconds / 60))" "$((seconds % 60))"
}

run_installation() {
    local log
    local stage_file
    local pid
    local frame=0
    local current_stage=""
    local shown_stage=""
    local started=$SECONDS
    local label
    local frames=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')

    if [[ "$ECLIPSE_DEMO" == true ]]; then
        for current_stage in \
            "Installing frontend dependencies" \
            "Building frontend" \
            "Building Eclipse backend… 01:42" \
            "Preparing runtime"; do
            printf '%s⠋%s %s\n' "$ECLIPSE_BLUE" "$ECLIPSE_RESET" "$current_stage"
        done
        success "Eclipse installed"
        return 0
    fi

    log=$(mktemp "${TMPDIR:-/tmp}/eclipse-install.XXXXXX")
    stage_file=$(mktemp "${TMPDIR:-/tmp}/eclipse-stage.XXXXXX")
    ECLIPSE_LOG=$log
    "$ECLIPSE_ROOT/scripts/bootstrap.sh" --release --stage-file "$stage_file" >"$log" 2>&1 &
    pid=$!
    ECLIPSE_STEP_PID=$pid

    while kill -0 "$pid" 2>/dev/null; do
        if [[ -s "$stage_file" ]]; then
            current_stage=$(head -n 1 "$stage_file")
        fi
        label=$current_stage
        if [[ "$current_stage" == "Building Eclipse backend" ]]; then
            label="${current_stage}… $(format_elapsed "$((SECONDS - started))")"
        fi
        if [[ -t 1 ]]; then
            printf '\r%s%s%s %s' "$ECLIPSE_BLUE" "${frames[$frame]}" "$ECLIPSE_RESET" "${label:-Preparing installation}"
            frame=$(((frame + 1) % ${#frames[@]}))
        elif [[ -n "$label" && "$label" != "$shown_stage" ]]; then
            notice "$label"
            shown_stage=$label
        fi
        sleep 0.1
    done
    [[ ! -t 1 ]] || printf '\r\033[2K'

    if wait "$pid"; then
        ECLIPSE_STEP_PID=""
        rm -f "$log" "$stage_file"
        ECLIPSE_LOG=""
        success "Eclipse installed"
        return 0
    fi

    ECLIPSE_STEP_PID=""
    failure "Eclipse installation failed."
    if [[ -s "$log" ]]; then
        printf '\n%sDiagnostic output:%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET" >&2
        tail -n 30 "$log" >&2
    fi
    printf '\n%sFull log:%s %s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET" "$log" >&2
    rm -f "$stage_file"
    return 1
}

force_stop_windows_processes() {
    local joined=""
    local pid
    for pid in "${ECLIPSE_RUNNING_PIDS[@]}"; do
        joined="${joined:+$joined,}$pid"
    done
    ECLIPSE_PROCESS_IDS="$joined" powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '
        $ids = $env:ECLIPSE_PROCESS_IDS.Split(",") | ForEach-Object { [int]$_ }
        Get-Process -Id $ids -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction Stop
    '
}

stop_running_eclipse() {
    local context=${1:-replace}
    local pid
    [[ ${#ECLIPSE_RUNNING_PIDS[@]} -gt 0 ]] || return 0

    confirm "Stop the running Eclipse process now?" || exit_without_changes
    ECLIPSE_SHUTDOWN_ATTEMPTED=true
    lifecycle_debug "shutdown confirmed for verified pids=${ECLIPSE_RUNNING_PIDS[*]}"
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        if [[ "$context" == clean ]]; then
            success "Eclipse stopped"
        else
            success "Running Eclipse stopped before replacement"
        fi
        ECLIPSE_RUNNING_PIDS=()
        return 0
    fi

    if [[ "$ECLIPSE_SELECTED_PLATFORM" == windows ]]; then
        lifecycle_debug "sending runtime-local graceful shutdown request path=$ECLIPSE_RUNTIME_DIR/eclipse.shutdown"
        if ! printf '%s\n' "shutdown" > "$ECLIPSE_RUNTIME_DIR/eclipse.shutdown"; then
            failure "The runtime-local Eclipse shutdown request could not be written."
            lifecycle_debug "runtime-local graceful shutdown request write failed"
            surface_lifecycle_diagnostics
            return 1
        fi
        lifecycle_debug "runtime-local graceful shutdown request written successfully"
    else
        for pid in "${ECLIPSE_RUNNING_PIDS[@]}"; do
            kill -TERM "$pid" 2>/dev/null || true
        done
    fi

    if wait_for_processes_to_stop; then
        if [[ "$context" == clean ]]; then
            success "Eclipse stopped"
        else
            success "Running Eclipse stopped before replacement"
        fi
        return 0
    fi

    warning "Eclipse did not finish shutting down within 20 seconds."
    lifecycle_debug "graceful shutdown timed out; explicit force-stop confirmation required"
    confirm "Force stop only the verified Eclipse process(es) for this installation?" || {
        failure "Eclipse is still running; installation cannot safely continue."
        lifecycle_debug "force-stop declined; destructive lifecycle aborted"
        surface_lifecycle_diagnostics
        return 1
    }
    if ! detect_running_eclipse; then
        lifecycle_debug "verified process exited before force-stop was issued"
        return 0
    fi
    lifecycle_debug "force-stopping reverified pids=${ECLIPSE_RUNNING_PIDS[*]}"
    if [[ "$ECLIPSE_SELECTED_PLATFORM" == windows ]]; then
        if ! force_stop_windows_processes; then
            failure "The verified Eclipse process could not be force-stopped."
            lifecycle_debug "verified force-stop command failed pids=${ECLIPSE_RUNNING_PIDS[*]}"
            surface_lifecycle_diagnostics
            return 1
        fi
    else
        for pid in "${ECLIPSE_RUNNING_PIDS[@]}"; do
            kill -KILL "$pid" 2>/dev/null || true
        done
    fi
    wait_for_processes_to_stop || {
        failure "The verified Eclipse process could not be stopped."
        lifecycle_debug "verified process still alive after force-stop"
        surface_lifecycle_diagnostics
        return 1
    }
    lifecycle_debug "verified process exited after force-stop"
    if [[ "$context" == clean ]]; then
        success "Eclipse stopped"
    else
        success "Running Eclipse stopped before replacement"
    fi
}

read_runtime_setting() {
    local key=$1
    local config="$ECLIPSE_RUNTIME_DIR/config/config.toml"
    [[ -f "$config" ]] || return 1
    awk -F= -v key="$key" '
        $1 ~ "^[[:space:]]*" key "[[:space:]]*$" {
            value = $2
            sub(/#.*/, "", value)
            gsub(/^[[:space:]\"]+|[[:space:]\"]+$/, "", value)
            print value
            exit
        }
    ' "$config"
}

configured_runtime_path() {
    local value=$1
    if [[ "$value" == /* ]]; then
        printf '%s\n' "$value"
    elif [[ "$value" =~ ^[A-Za-z]:[\\/] ]]; then
        if command_available cygpath; then cygpath -u "$value"; else printf '%s\n' "$value"; fi
    else
        printf '%s/%s\n' "$ECLIPSE_RUNTIME_DIR" "$value"
    fi
}

remove_managed_path() {
    local path=$1
    local runtime_absolute
    local parent_absolute
    local absolute
    [[ -e "$path" || -L "$path" ]] || return 0
    runtime_absolute=$(cd "$ECLIPSE_RUNTIME_DIR" && pwd -P)
    parent_absolute=$(cd "$(dirname "$path")" 2>/dev/null && pwd -P) || {
        warning "Preserved unresolvable path: $path"
        return 0
    }
    absolute="$parent_absolute/$(basename "$path")"
    case "$absolute" in
        "$runtime_absolute"/*)
            [[ "$absolute" != "$runtime_absolute" ]] || {
                failure "Refusing to remove the runtime root."
                return 1
            }
            rm -rf -- "$absolute"
            ;;
        *) warning "Preserved externally configured path: $path" ;;
    esac
}

queue_managed_path() {
    local path=$1
    local runtime_absolute
    local parent_absolute
    local absolute
    local queued
    [[ -e "$path" || -L "$path" ]] || return 0
    runtime_absolute=$(cd "$ECLIPSE_RUNTIME_DIR" && pwd -P)
    parent_absolute=$(cd "$(dirname "$path")" 2>/dev/null && pwd -P) || {
        warning "Preserved unresolvable path: $path"
        return 0
    }
    absolute="$parent_absolute/$(basename "$path")"
    case "$absolute" in
        "$runtime_absolute"/*)
            [[ "$absolute" != "$runtime_absolute" ]] || {
                failure "Refusing to remove the runtime root."
                return 1
            }
            if [[ ${#ECLIPSE_MANAGED_PATHS[@]} -gt 0 ]]; then
                for queued in "${ECLIPSE_MANAGED_PATHS[@]}"; do
                    [[ "$queued" == "$absolute" ]] && return 0
                done
            fi
            ECLIPSE_MANAGED_PATHS+=("$absolute")
            ;;
        *) warning "Preserved externally configured path: $path" ;;
    esac
}

collect_reset_boundary() {
    local configured_cache
    local config_temp
    configured_cache=$(read_runtime_setting cache_dir 2>/dev/null || true)
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/config/config.toml"
    for config_temp in "$ECLIPSE_RUNTIME_DIR"/config/.config.toml.tmp-*; do
        queue_managed_path "$config_temp"
    done
    [[ -n "$configured_cache" ]] && queue_managed_path "$(configured_runtime_path "$configured_cache")"
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/streaming_cache"
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/logs"
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/eclipse.log"
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/eclipse.pid"
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/eclipse.shutdown"
}

collect_clean_boundary() {
    local configured_metadata
    local configured_cache
    configured_metadata=$(read_runtime_setting metadata_dir 2>/dev/null || true)
    configured_cache=$(read_runtime_setting cache_dir 2>/dev/null || true)
    collect_reset_boundary
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/config/dim.db"
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/config/dim.db-journal"
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/config/dim.db-wal"
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/config/dim.db-shm"
    [[ -n "$configured_metadata" ]] && queue_managed_path "$(configured_runtime_path "$configured_metadata")"
    [[ -n "$configured_cache" ]] && queue_managed_path "$(configured_runtime_path "$configured_cache")"
    queue_managed_path "$ECLIPSE_RUNTIME_DIR/metadata"
}

verify_windows_managed_paths_released() {
    local joined=""
    local path
    local windows_path
    local diagnostic
    if [[ ${#ECLIPSE_MANAGED_PATHS[@]} -gt 0 ]]; then
        for path in "${ECLIPSE_MANAGED_PATHS[@]}"; do
            windows_path=$path
            if command_available cygpath; then
                windows_path=$(cygpath -w "$path")
            fi
            joined="${joined}${joined:+$'\n'}$windows_path"
        done
    fi
    [[ -n "$joined" ]] || return 0

    if ! diagnostic=$(ECLIPSE_CLEAN_TARGETS="$joined" powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '
        $failed = $false
        foreach ($target in ($env:ECLIPSE_CLEAN_TARGETS -split "`n")) {
            $item = Get-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
            if ($null -eq $item) { continue }
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { continue }
            $files = if ($item.PSIsContainer) {
                Get-ChildItem -LiteralPath $item.FullName -File -Force -Recurse -ErrorAction Stop |
                    Where-Object {
                        ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0
                    }
            } else {
                @($item)
            }
            foreach ($file in $files) {
                $stream = $null
                try {
                    $stream = [IO.File]::Open(
                        $file.FullName,
                        [IO.FileMode]::Open,
                        [IO.FileAccess]::ReadWrite,
                        [IO.FileShare]::None
                    )
                } catch {
                    [Console]::Error.WriteLine("$($file.FullName): $($_.Exception.Message)")
                    $failed = $true
                } finally {
                    if ($null -ne $stream) { $stream.Dispose() }
                }
            }
        }
        if ($failed) { exit 1 }
    ' 2>&1); then
        if [[ "$ECLIPSE_SHUTDOWN_ATTEMPTED" == true ]]; then
            failure "Existing Eclipse data is still in use after the verified process exited; no files were removed."
        else
            failure "No exact Eclipse process could be verified, but existing Eclipse data is locked; no files were removed."
        fi
        while IFS= read -r path; do
            [[ -z "$path" ]] || printf '  %s\n' "$path" >&2
        done <<< "$diagnostic"
        lifecycle_debug "exclusive-lock preflight failed shutdown_attempted=$ECLIPSE_SHUTDOWN_ATTEMPTED"
        while IFS= read -r path; do
            [[ -z "$path" ]] || lifecycle_debug "locked path=$path"
        done <<< "$diagnostic"
        surface_lifecycle_diagnostics
        return 1
    fi
    lifecycle_debug "exclusive-lock preflight succeeded for all managed files"
}

verify_clean_boundary_ready() {
    if detect_running_eclipse; then
        failure "Verified Eclipse process is still running (PID(s): ${ECLIPSE_RUNNING_PIDS[*]}). No files were removed."
        lifecycle_debug "clean preflight blocked by running verified pids=${ECLIPSE_RUNNING_PIDS[*]}"
        surface_lifecycle_diagnostics
        return 1
    fi
    lifecycle_debug "no exact Eclipse process remains before exclusive-lock preflight"
    if [[ "$ECLIPSE_SELECTED_PLATFORM" == windows ]]; then
        verify_windows_managed_paths_released
    fi
}

remove_collected_managed_paths() {
    local path
    [[ ${#ECLIPSE_MANAGED_PATHS[@]} -gt 0 ]] || return 0
    for path in "${ECLIPSE_MANAGED_PATHS[@]}"; do
        rm -rf -- "$path"
    done
}

apply_reset_boundary() {
    ECLIPSE_MANAGED_PATHS=()
    collect_reset_boundary
    remove_collected_managed_paths
}

apply_clean_boundary() {
    ECLIPSE_MANAGED_PATHS=()
    collect_clean_boundary
    notice "Checking existing data"
    verify_clean_boundary_ready
    success "Existing data ready for removal"
    notice "Removing existing Eclipse data"
    remove_collected_managed_paths
    success "Existing Eclipse data removed"
}

apply_installation_lifecycle() {
    if [[ "$ECLIPSE_INSTALL_MODE" == clean ]]; then
        printf '\n%sPreparing clean install%s\n\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"
    fi
    if detect_running_eclipse; then
        warning "Running Eclipse detected (PID(s): ${ECLIPSE_RUNNING_PIDS[*]})."
        [[ "$ECLIPSE_INSTALL_MODE" != clean ]] || notice "Stopping Eclipse"
        stop_running_eclipse "$ECLIPSE_INSTALL_MODE"
    fi

    if [[ "$ECLIPSE_DEMO" == true ]]; then
        case "$ECLIPSE_INSTALL_MODE" in
            reset) success "Demo reset boundary simulated" ;;
            clean)
                notice "Checking existing data"
                success "Existing data ready for removal"
                notice "Removing existing Eclipse data"
                success "Existing Eclipse data removal simulated"
                ;;
        esac
        return 0
    fi

    case "$ECLIPSE_INSTALL_MODE" in
        reset) apply_reset_boundary ;;
        clean) apply_clean_boundary ;;
    esac

    remove_managed_path "$ECLIPSE_RUNTIME_DIR/dim$ECLIPSE_BINARY_SUFFIX"
}

node_is_supported() {
    command_available node && node -e '
        const [major, minor, patch] = process.versions.node.split(".").map(Number);
        process.exit(major === 24 && (minor > 19 || (minor === 19 && patch >= 0)) ? 0 : 1);
    ' >/dev/null 2>&1
}

classify_media_tool() {
    local tool=$1
    local version_line
    local major=""
    if ! command_available "$tool"; then
        printf 'missing|'
        return 0
    fi
    if ! version_line=$("$tool" -version 2>/dev/null | head -n 1); then
        printf 'invalid|'
        return 0
    fi
    if [[ "$version_line" =~ ^${tool}[[:space:]]version[[:space:]]n?([0-9]+)([.[:space:]]|$) ]]; then
        major=${BASH_REMATCH[1]}
    else
        printf 'invalid|'
        return 0
    fi
    if (( major < 9 )); then
        printf 'unsupported|%s' "$major"
    else
        printf 'valid|%s' "$major"
    fi
}

classify_media_toolchain() {
    local ffmpeg_result
    local ffprobe_result
    ffmpeg_result=$(classify_media_tool ffmpeg)
    ffprobe_result=$(classify_media_tool ffprobe)
    ECLIPSE_FFMPEG_STATUS=${ffmpeg_result%%|*}
    ECLIPSE_FFMPEG_MAJOR=${ffmpeg_result#*|}
    ECLIPSE_FFPROBE_STATUS=${ffprobe_result%%|*}
    ECLIPSE_FFPROBE_MAJOR=${ffprobe_result#*|}

    if [[ "$ECLIPSE_FFMPEG_STATUS" == valid && "$ECLIPSE_FFPROBE_STATUS" == valid ]]; then
        if [[ "$ECLIPSE_FFMPEG_MAJOR" == "$ECLIPSE_FFPROBE_MAJOR" ]]; then
            ECLIPSE_MEDIA_STATUS=valid
        else
            ECLIPSE_MEDIA_STATUS=mismatched
        fi
    elif [[ "$ECLIPSE_FFMPEG_STATUS" == missing || "$ECLIPSE_FFPROBE_STATUS" == missing ]]; then
        ECLIPSE_MEDIA_STATUS=missing
    elif [[ "$ECLIPSE_FFMPEG_STATUS" == unsupported || "$ECLIPSE_FFPROBE_STATUS" == unsupported ]]; then
        ECLIPSE_MEDIA_STATUS=unsupported
    else
        ECLIPSE_MEDIA_STATUS=invalid
    fi
}

media_toolchain_is_supported() {
    classify_media_toolchain
    [[ "$ECLIPSE_MEDIA_STATUS" == valid ]]
}

package_list_contains() {
    local needle=$1
    shift
    local package
    for package in "$@"; do
        [[ "$package" == "$needle" ]] && return 0
    done
    return 1
}

print_requirement_failure() {
    local item=$1
    case "$item" in
        xcode) printf '  • Apple Command Line Tools — run: xcode-select --install\n' ;;
        git) printf '  • Git — supplied by Apple Command Line Tools\n' ;;
        node) printf '  • Node.js 24.19.0 or newer in the 24.x line — Homebrew package: node@24\n' ;;
        corepack) printf '  • Corepack — included with supported Node.js; then run: corepack enable pnpm\n' ;;
        rustup) printf '  • Rustup and the repository-pinned Rust 1.93.1 toolchain — https://rustup.rs\n' ;;
        ffmpeg) printf '  • FFmpeg and FFprobe 9.0 or newer — Homebrew package: ffmpeg (ffmpeg@9)\n' ;;
        sqlite) printf '  • SQLite tools — Homebrew package: sqlite\n' ;;
        pkgconfig) printf '  • pkg-config — Homebrew package: pkg-config\n' ;;
        curl) printf '  • curl — supplied by macOS and needed to install Rustup\n' ;;
    esac
}

print_linux_requirement_failure() {
    local item=$1
    case "$item" in
        git) printf '  • Git — Debian/Ubuntu package: git\n' ;;
        node)
            if command_available node; then
                printf '  • Node.js 24.19.0 or newer in the 24.x line — found %s; install a supported Node.js 24 release from https://nodejs.org\n' "$(node --version 2>/dev/null || printf unknown)"
            else
                printf '  • Node.js 24.19.0 or newer in the 24.x line — install a supported Node.js 24 release from https://nodejs.org\n'
            fi
            ;;
        corepack) printf '  • Corepack — install it with supported Node.js, then run: corepack enable pnpm\n' ;;
        rustup) printf '  • Rustup and the repository-pinned Rust 1.93.1 toolchain — https://rustup.rs\n' ;;
        ffmpeg) printf '  • FFmpeg and FFprobe 9.0 or newer — Eclipse pinned, checksum-verified toolchain\n' ;;
        sqlite) printf '  • SQLite tools — Debian/Ubuntu package: sqlite3\n' ;;
        pkgconfig) printf '  • pkg-config — Debian/Ubuntu package: pkg-config\n' ;;
        buildtools) printf '  • C/C++ build toolchain — Debian/Ubuntu package: build-essential\n' ;;
        openssl) printf '  • OpenSSL development headers — Debian/Ubuntu package: libssl-dev\n' ;;
        curl) printf '  • curl — Debian/Ubuntu package: curl\n' ;;
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
    media_toolchain_is_supported || ECLIPSE_MISSING+=(ffmpeg)
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
        if brew --prefix ffmpeg >/dev/null 2>&1; then
            export PATH="$(brew --prefix ffmpeg)/bin:$PATH"
        fi
    fi
}

brew_install_or_upgrade_packages() {
    local package
    for package in "$@"; do
        if [[ "$package" == ffmpeg ]] && brew list --versions ffmpeg >/dev/null 2>&1; then
            brew upgrade ffmpeg
        else
            brew install "$package"
        fi
    done
}

install_homebrew_requirements() {
    local packages=()
    local item
    for item in "${ECLIPSE_MISSING[@]}"; do
        case "$item" in
            node|corepack)
                if [[ ${#packages[@]} -eq 0 ]] || ! package_list_contains node@24 "${packages[@]}"; then packages+=(node@24); fi
                ;;
            ffmpeg)
                if [[ ${#packages[@]} -eq 0 ]] || ! package_list_contains ffmpeg "${packages[@]}"; then packages+=(ffmpeg); fi
                ;;
            sqlite)
                if [[ ${#packages[@]} -eq 0 ]] || ! package_list_contains sqlite "${packages[@]}"; then packages+=(sqlite); fi
                ;;
            pkgconfig)
                if [[ ${#packages[@]} -eq 0 ]] || ! package_list_contains pkg-config "${packages[@]}"; then packages+=(pkg-config); fi
                ;;
        esac
    done
    if [[ ${#packages[@]} -gt 0 ]]; then
        if [[ "$ECLIPSE_DEMO" == false ]]; then
            command_available brew || return 1
        fi
        confirm "Install missing Homebrew packages (${packages[*]})?" || return 1
        run_step "Installed Homebrew requirements" brew_install_or_upgrade_packages "${packages[@]}" || return 1
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
        if brew --prefix ffmpeg >/dev/null 2>&1; then
            export PATH="$(brew --prefix ffmpeg)/bin:$PATH"
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
    run_step "System requirements ready" rustup toolchain install 1.93.1 --profile minimal --component rustfmt --component clippy
}

collect_linux_requirements() {
    ECLIPSE_MISSING=()
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        if [[ "$ECLIPSE_DEMO_REQUIREMENTS_RESOLVED" == false ]]; then
            ECLIPSE_MISSING=(ffmpeg pkgconfig)
        fi
        return 0
    fi
    command_available git || ECLIPSE_MISSING+=(git)
    node_is_supported || ECLIPSE_MISSING+=(node)
    command_available corepack || ECLIPSE_MISSING+=(corepack)
    if ! command_available rustup || ! command_available cargo || ! command_available rustc; then ECLIPSE_MISSING+=(rustup); fi
    media_toolchain_is_supported || ECLIPSE_MISSING+=(ffmpeg)
    command_available sqlite3 || ECLIPSE_MISSING+=(sqlite)
    command_available pkg-config || ECLIPSE_MISSING+=(pkgconfig)
    if ! command_available cc || ! command_available c++; then ECLIPSE_MISSING+=(buildtools); fi
    if ! command_available pkg-config || ! pkg-config --exists openssl >/dev/null 2>&1; then ECLIPSE_MISSING+=(openssl); fi
    command_available curl || ECLIPSE_MISSING+=(curl)
}

apt_install() {
    if [[ $EUID -eq 0 ]]; then
        apt-get update
        apt-get install -y "$@"
    else
        sudo -n apt-get update
        sudo -n apt-get install -y "$@"
    fi
}

install_apt_requirements() {
    local packages=()
    local item
    local package
    for item in "${ECLIPSE_MISSING[@]}"; do
        package=""
        case "$item" in
            git) package=git ;;
            sqlite) package=sqlite3 ;;
            pkgconfig) package=pkg-config ;;
            buildtools) package=build-essential ;;
            openssl) package=libssl-dev ;;
            curl) package=curl ;;
        esac
        [[ -n "$package" ]] || continue
        if [[ ${#packages[@]} -eq 0 ]] || ! package_list_contains "$package" "${packages[@]}"; then
            packages+=("$package")
        fi
    done

    [[ ${#packages[@]} -gt 0 ]] || return 0
    if [[ "$ECLIPSE_DEMO" == false ]]; then
        command_available apt-get || return 1
        if [[ $EUID -ne 0 ]] && ! command_available sudo; then
            return 1
        fi
    fi
    confirm "Install missing Debian/Ubuntu packages (${packages[*]})?" || return 1
    if [[ "$ECLIPSE_DEMO" == false && $EUID -ne 0 ]]; then
        notice "Administrator access is required to install system packages"
        if ! sudo -v; then
            failure "Administrator access was not granted. No packages were installed."
            return 1
        fi
    fi
    run_step "Installed Linux requirements" apt_install "${packages[@]}" || return 1
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        return 0
    fi
}

linux_media_repair_needed() {
    local item
    for item in "${ECLIPSE_MISSING[@]}"; do
        [[ "$item" == ffmpeg ]] && return 0
    done
    return 1
}

install_linux_media_tools() {
    linux_media_repair_needed || return 0
    confirm "Install Eclipse's pinned FFmpeg 9 toolchain?" || return 1
    run_step "Installed Eclipse FFmpeg 9 toolchain" \
        bash "$ECLIPSE_ROOT/scripts/install-ffmpeg9-linux.sh" "$ECLIPSE_ROOT/utils" || return 1
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        ECLIPSE_DEMO_REQUIREMENTS_RESOLVED=true
    else
        export PATH="$ECLIPSE_ROOT/utils:$PATH"
    fi
}

resolve_linux_requirements() {
    local apt_needed=false
    local item
    for item in "${ECLIPSE_MISSING[@]}"; do
        case "$item" in git|sqlite|pkgconfig|buildtools|openssl|curl) apt_needed=true ;; esac
    done

    if [[ "$ECLIPSE_DEMO" == false && "$apt_needed" == true ]] && ! command_available apt-get; then
        printf '\n'
        failure "Automatic dependency recovery currently supports Debian and Ubuntu (apt-get)."
        printf 'Install the equivalent packages for this distribution, then run:\n  %s\n\nMissing requirements:\n' "$ECLIPSE_ROOT/install.sh"
        for item in "${ECLIPSE_MISSING[@]}"; do print_linux_requirement_failure "$item"; done
        return 1
    fi

    if [[ "$ECLIPSE_DEMO" == false && "$apt_needed" == true && $EUID -ne 0 ]] && ! command_available sudo; then
        printf '\n'
        failure "Installing Debian/Ubuntu packages requires root access or sudo."
        printf 'Install the packages below as root, then run:\n  %s\n\nMissing requirements:\n' "$ECLIPSE_ROOT/install.sh"
        for item in "${ECLIPSE_MISSING[@]}"; do print_linux_requirement_failure "$item"; done
        return 1
    fi

    install_apt_requirements || true
    install_linux_media_tools || true
    install_rustup || true
    collect_linux_requirements
    if [[ ${#ECLIPSE_MISSING[@]} -gt 0 ]]; then
        printf '\n'
        failure "Some requirements still need attention:"
        for item in "${ECLIPSE_MISSING[@]}"; do print_linux_requirement_failure "$item"; done
        printf '\nAfter resolving them, run:\n  %s\n' "$ECLIPSE_ROOT/install.sh"
        return 1
    fi
}

check_linux_requirements() {
    printf '%sChecking requirements%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"
    collect_linux_requirements
    if [[ ${#ECLIPSE_MISSING[@]} -gt 0 ]]; then
        warning "Found ${#ECLIPSE_MISSING[@]} missing or unsupported requirement(s)."
        resolve_linux_requirements || return 1
    fi
    run_step "System requirements ready" rustup toolchain install 1.93.1 --profile minimal --component rustfmt --component clippy
}

detect_windows_toolchain() {
    local detector="$ECLIPSE_ROOT/scripts/windows-toolchain.ps1"
    local result
    local status
    if ! command_available powershell.exe; then
        ECLIPSE_WINDOWS_TOOLCHAIN_DETAIL="PowerShell is unavailable, so the MSVC compiler and Windows SDK could not be checked"
        ECLIPSE_WINDOWS_TOOLCHAIN_STATUS="toolchain-inconclusive"
        return 0
    fi
    if command_available cygpath; then
        detector=$(cygpath -w "$detector")
    fi
    if ! result=$(MSYS2_ARG_CONV_EXCL='*' powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$detector" 2>/dev/null); then
        ECLIPSE_WINDOWS_TOOLCHAIN_DETAIL="The Windows toolchain detector could not run"
        ECLIPSE_WINDOWS_TOOLCHAIN_STATUS="toolchain-inconclusive"
        return 0
    fi
    result=${result//$'\r'/}
    status=${result%%|*}
    if [[ "$result" == *"|"* ]]; then
        ECLIPSE_WINDOWS_TOOLCHAIN_DETAIL=${result#*|}
    else
        ECLIPSE_WINDOWS_TOOLCHAIN_DETAIL="The Windows toolchain detector returned an unexpected result"
    fi
    case "$status" in
        ready) ECLIPSE_WINDOWS_TOOLCHAIN_STATUS="ready" ;;
        missing-build-tools) ECLIPSE_WINDOWS_TOOLCHAIN_STATUS="buildtools" ;;
        missing-vctools) ECLIPSE_WINDOWS_TOOLCHAIN_STATUS="vctools" ;;
        missing-sdk) ECLIPSE_WINDOWS_TOOLCHAIN_STATUS="windowssdk" ;;
        *) ECLIPSE_WINDOWS_TOOLCHAIN_STATUS="toolchain-inconclusive" ;;
    esac
}

refresh_windows_path() {
    [[ "$ECLIPSE_DEMO" == false ]] || return 0
    local windows_paths=(
        "${CARGO_HOME:-$HOME/.cargo}/bin"
        "/c/Program Files/nodejs"
        "/c/Program Files/Git/cmd"
    )
    if command_available cygpath; then
        if [[ -n "${LOCALAPPDATA:-}" ]]; then
            windows_paths+=("$(cygpath -u "$LOCALAPPDATA")/Microsoft/WinGet/Links")
        fi
    fi
    local path
    for path in "${windows_paths[@]}"; do
        [[ -d "$path" ]] && export PATH="$path:$PATH"
    done
    return 0
}

prepare_windows_pnpm_command() {
    local script="$ECLIPSE_ROOT/scripts/prepare-pnpm.ps1"
    local repository_root=$ECLIPSE_ROOT
    if command_available cygpath; then
        script=$(cygpath -w "$script")
        repository_root=$(cygpath -w "$repository_root")
    fi
    MSYS2_ARG_CONV_EXCL='*' powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass \
        -File "$script" -RepositoryRoot "$repository_root"
}

collect_windows_requirements() {
    ECLIPSE_MISSING=()
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        if [[ "$ECLIPSE_DEMO_REQUIREMENTS_RESOLVED" == false ]]; then
            ECLIPSE_MISSING=(ffmpeg buildtools)
        fi
        return 0
    fi
    command_available git || ECLIPSE_MISSING+=(git)
    node_is_supported || ECLIPSE_MISSING+=(node)
    command_available corepack || ECLIPSE_MISSING+=(corepack)
    if ! command_available rustup || ! command_available cargo || ! command_available rustc; then ECLIPSE_MISSING+=(rustup); fi
    media_toolchain_is_supported || ECLIPSE_MISSING+=(ffmpeg)
    command_available sqlite3 || ECLIPSE_MISSING+=(sqlite)
    detect_windows_toolchain
    [[ "$ECLIPSE_WINDOWS_TOOLCHAIN_STATUS" == ready ]] || ECLIPSE_MISSING+=("$ECLIPSE_WINDOWS_TOOLCHAIN_STATUS")
    command_available curl || ECLIPSE_MISSING+=(curl)
}

print_windows_requirement_failure() {
    local item=$1
    case "$item" in
        git) printf '  • Git for Windows — WinGet package: Git.Git\n' ;;
        node)
            if command_available node; then
                printf '  • Node.js 24.19.0 or newer in the 24.x line — found %s; WinGet package: OpenJS.NodeJS.LTS\n' "$(node --version 2>/dev/null || printf unknown)"
            else
                printf '  • Node.js 24.19.0 or newer in the 24.x line — WinGet package: OpenJS.NodeJS.LTS\n'
            fi
            ;;
        corepack) printf '  • Corepack — reinstall supported Node.js; the installer prepares the pnpm command once Corepack is available\n' ;;
        rustup) printf '  • Rustup and the repository-pinned Rust 1.93.1 toolchain — WinGet package: Rustlang.Rustup\n' ;;
        ffmpeg) printf '  • FFmpeg and FFprobe 9.0 or newer — WinGet package: Gyan.FFmpeg\n' ;;
        sqlite) printf '  • SQLite tools — WinGet package: SQLite.SQLite\n' ;;
        buildtools) printf '  • Visual Studio 2022 C++ Build Tools, MSVC and Windows SDK were not found — WinGet package: Microsoft.VisualStudio.2022.BuildTools\n' ;;
        vctools) printf '  • Visual Studio is installed, but the MSVC x64 compiler/VCTools component is missing. In Visual Studio Installer, choose Modify and add Desktop development with C++.\n' ;;
        windowssdk) printf '  • MSVC is installed, but a usable Windows SDK is missing. In Visual Studio Installer, choose Modify and add a current Windows SDK.\n' ;;
        toolchain-inconclusive) printf '  • Visual Studio detection was inconclusive — %s. Diagnose with: powershell -NoProfile -File .\\scripts\\windows-toolchain.ps1\n' "$ECLIPSE_WINDOWS_TOOLCHAIN_DETAIL" ;;
        curl) printf '  • curl — included with current Windows releases; restore it or add curl.exe to PATH\n' ;;
    esac
}

winget_install_packages() {
    local package
    for package in "$@"; do
        case "$package" in
            OpenJS.NodeJS.LTS)
                winget install --id "$package" --exact --version 24.19.0 --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity
                ;;
            Microsoft.VisualStudio.2022.BuildTools)
                winget install --id "$package" --exact --source winget --accept-package-agreements --accept-source-agreements --override "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
                ;;
            Gyan.FFmpeg)
                if [[ "$ECLIPSE_MEDIA_STATUS" == missing ]]; then
                    winget install --id "$package" --exact --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity
                else
                    winget upgrade --id "$package" --exact --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity || \
                        winget install --id "$package" --exact --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity
                fi
                ;;
            *)
                winget install --id "$package" --exact --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity
                ;;
        esac
    done
}

install_winget_requirements() {
    local packages=()
    local item
    local package
    for item in "${ECLIPSE_MISSING[@]}"; do
        package=""
        case "$item" in
            git) package=Git.Git ;;
            node) package=OpenJS.NodeJS.LTS ;;
            rustup) package=Rustlang.Rustup ;;
            ffmpeg) package=Gyan.FFmpeg ;;
            sqlite) package=SQLite.SQLite ;;
            buildtools) package=Microsoft.VisualStudio.2022.BuildTools ;;
        esac
        [[ -n "$package" ]] || continue
        if [[ ${#packages[@]} -eq 0 ]] || ! package_list_contains "$package" "${packages[@]}"; then
            packages+=("$package")
        fi
    done

    [[ ${#packages[@]} -gt 0 ]] || return 0
    if [[ "$ECLIPSE_DEMO" == false ]] && ! command_available winget; then
        return 1
    fi
    confirm "Install missing Windows packages (${packages[*]})?" || return 1
    if [[ "$ECLIPSE_DEMO" == false ]]; then
        notice "Windows may request administrator approval for system packages"
    fi
    run_step "Installed Windows requirements" winget_install_packages "${packages[@]}" || return 1
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        ECLIPSE_DEMO_REQUIREMENTS_RESOLVED=true
    else
        refresh_windows_path
    fi
}

resolve_windows_requirements() {
    local recoverable=false
    local item
    for item in "${ECLIPSE_MISSING[@]}"; do
        case "$item" in git|node|rustup|ffmpeg|sqlite|buildtools) recoverable=true ;; esac
    done

    if [[ "$ECLIPSE_DEMO" == false && "$recoverable" == true ]] && ! command_available winget; then
        printf '\n'
        failure "Automatic Windows recovery requires WinGet."
        printf 'Install or update Microsoft App Installer, then run this command again:\n  install.cmd\n\nMissing requirements:\n'
        for item in "${ECLIPSE_MISSING[@]}"; do print_windows_requirement_failure "$item"; done
        return 1
    fi

    install_winget_requirements || true
    collect_windows_requirements
    if [[ ${#ECLIPSE_MISSING[@]} -gt 0 ]]; then
        printf '\n'
        failure "Some requirements still need attention:"
        for item in "${ECLIPSE_MISSING[@]}"; do print_windows_requirement_failure "$item"; done
        printf '\nWinGet installations can require a new terminal before PATH changes are visible. Open a new CMD or PowerShell window, then run:\n  install.cmd\n'
        return 1
    fi
}

check_windows_requirements() {
    printf '%sChecking requirements%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"
    collect_windows_requirements
    if [[ ${#ECLIPSE_MISSING[@]} -gt 0 ]]; then
        warning "Found ${#ECLIPSE_MISSING[@]} missing or unsupported requirement(s)."
        resolve_windows_requirements || return 1
    fi
    run_step "pnpm command ready in new terminals" prepare_windows_pnpm_command
    run_step "System requirements ready" rustup toolchain install 1.93.1 --profile minimal --component rustfmt --component clippy
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

launch_eclipse_and_wait() {
    local runtime_dir="$ECLIPSE_ROOT/target/release"
    local log="$runtime_dir/eclipse.log"
    local pid

    nohup "$ECLIPSE_ROOT/scripts/run.sh" --release >"$log" 2>&1 &
    pid=$!
    printf '%s\n' "$pid" > "$runtime_dir/eclipse.pid"
    if wait_for_eclipse "$pid"; then
        return 0
    fi

    printf 'Eclipse did not become ready at %s.\n' "$ECLIPSE_URL" >&2
    if [[ -s "$log" ]]; then
        printf '\nRecent Eclipse output:\n' >&2
        tail -n 30 "$log" >&2
    fi
    printf '\nRuntime log: %s\n' "$log" >&2
    return 1
}

start_eclipse() {
    printf '%sStarting Eclipse%s\n\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"

    if [[ "$ECLIPSE_DEMO" == true ]]; then
        run_step "Eclipse started" true
    elif curl --fail --silent "$ECLIPSE_URL/health/ready" >/dev/null 2>&1; then
        success "Eclipse is already running"
    else
        run_step "Eclipse started" launch_eclipse_and_wait
    fi

    printf '\n%sEclipse is ready:%s %s%s%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET" "$ECLIPSE_BLUE" "$ECLIPSE_URL" "$ECLIPSE_RESET"
    if confirm "Open Eclipse in your default browser?"; then
        if [[ "$ECLIPSE_DEMO" == true ]]; then
            run_step "Opened Eclipse" true
        elif open_browser "$ECLIPSE_URL"; then
            success "Opened Eclipse"
        else
            warning "The browser could not be opened automatically. Open $ECLIPSE_URL yourself."
        fi
    fi
}

open_browser() {
    local url=$1
    case "$ECLIPSE_SELECTED_PLATFORM" in
        macos)
            command_available open && open "$url"
            ;;
        linux)
            command_available xdg-open && xdg-open "$url"
            ;;
        windows)
            command_available cmd.exe && MSYS2_ARG_CONV_EXCL='*' cmd.exe /c start "" "$url"
            ;;
        *) return 1 ;;
    esac
}

install_and_offer_start() {
    apply_installation_lifecycle
    printf '\n%sInstalling Eclipse%s\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"
    run_installation
    if [[ -n "$ECLIPSE_LIFECYCLE_LOG" ]]; then
        rm -f "$ECLIPSE_LIFECYCLE_LOG"
        ECLIPSE_LIFECYCLE_LOG=""
    fi
    case "$ECLIPSE_INSTALL_MODE" in
        reinstall) success "Existing configuration preserved" ;;
        reset) success "Accounts, libraries, and metadata preserved" ;;
        clean) success "Managed Eclipse data reset for a clean first run" ;;
    esac
    use_configured_local_url

    printf '\n%sEclipse is ready.%s\n\n' "$ECLIPSE_BOLD" "$ECLIPSE_RESET"
    if [[ "$ECLIPSE_START" == true ]]; then
        select_menu "" true "Start Eclipse" "Exit"
    else
        ECLIPSE_MENU_SELECTION=1
    fi

    if [[ $ECLIPSE_MENU_SELECTION -eq 0 ]]; then
        start_eclipse
    else
        if [[ "$ECLIPSE_SELECTED_PLATFORM" == windows ]]; then
            printf 'Eclipse was not started. Start it later from CMD or PowerShell with:\n  install.cmd --platform windows --yes\n'
        else
            printf 'Eclipse was not started. Start it later with:\n  %s --release\n' "$ECLIPSE_ROOT/scripts/run.sh"
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
    printf '\n'
    prepare_installation_lifecycle
    prepare_macos_path
    check_macos_requirements
    install_and_offer_start
}

install_platform_linux() {
    if [[ "$ECLIPSE_DEMO" == false && $(uname -s) != Linux ]]; then
        failure "The Linux installer must be run on Linux. You selected Linux, but this system reports $(uname -s)."
        return 1
    fi

    notice "Linux selected"
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        notice "Demo mode — all checks and actions are simulated"
    fi
    printf '\n'
    prepare_installation_lifecycle
    if [[ -d "${CARGO_HOME:-$HOME/.cargo}/bin" && "$ECLIPSE_DEMO" == false ]]; then
        export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
    fi
    check_linux_requirements
    install_and_offer_start
}

install_platform_windows() {
    if [[ "$ECLIPSE_DEMO" == false ]]; then
        case "$(uname -s)" in
            MINGW*|MSYS*|CYGWIN*) ;;
            *)
                failure "Launch the Windows installer with install.cmd from CMD or PowerShell, or run install.sh from Git Bash. WSL is not required or supported."
                return 1
                ;;
        esac
    fi

    notice "Windows selected"
    if [[ "$ECLIPSE_DEMO" == true ]]; then
        notice "Demo mode — all checks and actions are simulated"
    fi
    printf '\n'
    prepare_installation_lifecycle
    refresh_windows_path
    check_windows_requirements
    install_and_offer_start
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
