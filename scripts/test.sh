#!/usr/bin/env bash

set -euo pipefail

ECLIPSE_ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
exec node "$ECLIPSE_ROOT_DIR/scripts/task.mjs" test "$@"
