#!/bin/sh

set -eu

runtime=/opt/eclipse
if grep -Eq '[[:space:]]/opt/dim/(config|metadata|streaming_cache|logs)[[:space:]]' /proc/self/mountinfo 2>/dev/null ||
   [ -f /opt/dim/config/config.toml ] || [ -f /opt/dim/config/dim.db ]; then
    runtime=/opt/dim
    echo "Using the legacy /opt/dim persistent runtime layout for compatibility." >&2
fi

cd "$runtime"
exec /opt/eclipse/eclipse "$@"
