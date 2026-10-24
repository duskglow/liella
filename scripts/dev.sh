#!/bin/sh
# Run a cargo command for this workspace from Linux or WSL, keeping build output on a
# Linux-native filesystem (a OneDrive-synced /mnt/c checkout is slow and sync-hostile).
#   scripts/dev.sh xtask test        # policy checks + all suites
#   scripts/dev.sh xtask stubs       # regenerate pending stubs from the spec
#   scripts/dev.sh xtask status      # pending vs specified tests per section
#   scripts/dev.sh test -p liella     # any cargo command
set -eu
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/liella-target}"
exec cargo "$@"
