#!/usr/bin/env bash
set -euo pipefail

# Release packaging uses an immutable FFmpeg 9 build. Source installs may use a
# distro package, but scripts/build.mjs applies the same major-version gate.
destination=${1:?usage: install-ffmpeg9-linux.sh DESTINATION [amd64|arm64]}
architecture=${2:-$(uname -m)}
release=autobuild-2026-08-20-13-45
version=n9.0.1-6-g9d4ca21220

case "$architecture" in
    amd64|x86_64)
        target=linux64
        checksum=8d2db9b783a161e92ed905b70e1a5e449172aefac369635809bc07fe30165950
        ;;
    arm64|aarch64)
        target=linuxarm64
        checksum=3f6345e8e816a0e717c8295c1797936164a48eb4c46af78bb6740f96b21999dd
        ;;
    *)
        printf 'No verified FFmpeg 9 Eclipse toolchain is provisioned for Linux architecture %s.\n' "$architecture" >&2
        exit 2
        ;;
esac

archive="ffmpeg-${version}-${target}-gpl-9.0.tar.xz"
url="https://github.com/BtbN/FFmpeg-Builds/releases/download/${release}/${archive}"
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT

curl -fsSL "$url" -o "$work/$archive"
printf '%s  %s\n' "$checksum" "$work/$archive" | sha256sum -c -
mkdir -p "$work/extracted" "$destination"
tar -xJf "$work/$archive" -C "$work/extracted"

ffmpeg=$(find "$work/extracted" -type f -path '*/bin/ffmpeg' -print -quit)
ffprobe=$(find "$work/extracted" -type f -path '*/bin/ffprobe' -print -quit)
[[ -n "$ffmpeg" && -n "$ffprobe" ]] || {
    printf 'The verified FFmpeg archive did not contain ffmpeg and ffprobe.\n' >&2
    exit 3
}
install -m 0755 "$ffmpeg" "$destination/ffmpeg"
install -m 0755 "$ffprobe" "$destination/ffprobe"

for tool in ffmpeg ffprobe; do
    version_line=$("$destination/$tool" -version | head -n 1)
    if [[ ! "$version_line" =~ ^${tool}[[:space:]]version[[:space:]]n?([0-9]+) ]] || (( BASH_REMATCH[1] < 9 )); then
        printf 'Provisioned %s is unsupported: %s\n' "$tool" "$version_line" >&2
        exit 4
    fi
    printf '%s\n' "$version_line"
done
