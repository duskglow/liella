#!/bin/bash
# FS-TMPFS-SCRATCH-RUN: does a test pass or fail depending on the file system under its scratch tree?
#
# Runs the tests of `liella` (crates/core) and `liella-boundary` (crates/boundary) twice, each inside an unprivileged
# user + mount namespace (`unshare -r -m`: no sudo, nothing on the host changes):
#   base run : a directory of the reference file system (ext4 on our hosts, --base-dir) is bind-mounted over `target/tmp`
#              and `/tmp` (a bind mount, because target/ itself may be on a RAM disk, as on keke);
#   tmpfs run: a fresh tmpfs (RAM) is mounted over the same two places.
# `target/tmp` is CARGO_TARGET_TMPDIR, baked into the tests at compile time, so only a mount can move it; TMPDIR=/tmp.
# The mounts are made as root of the first namespace; the tests then run in a second user namespace that maps the
# caller's own uid and gid to themselves (no capabilities, the same ids as a plain run). Both runs are built alike, so
# the `sudo -n` tests find no setuid helper in either and report their skip line: they are not covered by this check
# (docs/testing-filesystems.md). The sorted pass/fail lists are then compared: a difference is a test that depends on
# the file system (or a flaky one: rerun the named tests alone before blaming the file system).
#
# usage: validation/tmpfs-run.sh [--out DIR] [--base-dir D] [--no-build] [-- <extra nextest args, e.g. -E <filter>>]
#   --out DIR     logs and lists (default: $TMPFS_RUN_OUT, else a new directory under ${TMPDIR:-/tmp})
#   --base-dir D  directory on the reference file system (default $TMPFS_RUN_BASE or ~/.cache/tmpfs-run-base);
#                 the base run's scratch lands there and is left for the caller to remove
#   --no-build    skip `cargo xtask so` and the test build (products and test binaries already exist)
# exit: 0 lists identical; 1 lists differ; 2 the run itself broke (build failed, no namespace, no list, same fs type).
# Files in DIR: base.log tmpfs.log base.list tmpfs.list list.diff summary.txt
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
out=${TMPFS_RUN_OUT:-}
base=${TMPFS_RUN_BASE:-$HOME/.cache/tmpfs-run-base}
build=1
while [ $# -gt 0 ]; do
  case "$1" in
    --out) out=$2; shift 2 ;;
    --base-dir) base=$2; shift 2 ;;
    --no-build) build=0; shift ;;
    --) shift; break ;;
    *) echo "tmpfs-run.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done
extra=("$@")
[ -n "$out" ] || out=$(mktemp -d "${TMPDIR:-/tmp}/tmpfs-run.XXXXXX") || exit 2
mkdir -p "$out" "$base" || exit 2
base=$(cd "$base" && pwd)
cd "$root" || exit 2
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
mkdir -p target/tmp

if [ "$build" = 1 ]; then
  echo "== build (once, shared by both runs)"
  cargo xtask so > "$out/build.log" 2>&1 || { echo "build: cargo xtask so failed, see $out/build.log"; exit 2; }
  cargo nextest run --no-run -p liella -p liella-boundary >> "$out/build.log" 2>&1 \
    || { echo "build: test build failed, see $out/build.log"; exit 2; }
fi

# Runs inside the first namespace: $1 = base|tmpfs, $2 = log file; the rest are nextest arguments.
inner='
set -u
mode=$1; log=$2; shift 2
if [ "$mode" = tmpfs ]; then
  mount -t tmpfs -o size=6g tmpfs "$ROOT/target/tmp" || exit 2
  mount -t tmpfs -o size=1g tmpfs /tmp || exit 2
else
  mkdir -p "$BASE/target-tmp" "$BASE/tmp" || exit 2
  mount --bind "$BASE/target-tmp" "$ROOT/target/tmp" || exit 2
  mount --bind "$BASE/tmp" /tmp || exit 2
fi
exec unshare -U --map-user="$RUN_UID" --map-group="$RUN_GID" bash -c "$TEST_BODY" tmpfs-run-test "$mode" "$log" "$@"
'
# Runs in the second namespace, as the caller's own uid: $1 = mode, $2 = log, the rest nextest arguments.
export TEST_BODY='
mode=$1; log=$2; shift 2
export TMPDIR=/tmp
echo "scratch fs ($mode run): target/tmp=$(stat -f -c %T "$ROOT/target/tmp") /tmp=$(stat -f -c %T /tmp) uid=$(id -u)" > "$log"
cd "$ROOT" || exit 2
cargo nextest run -p liella -p liella-boundary --no-fail-fast "$@" >> "$log" 2>&1
echo "nextest exit status: $?" >> "$log"
'

# One line per test: verdict and name; durations and the "(n/total)" counter removed, the final-status repeats folded.
mklist() {
  grep -E '^[[:space:]]*(PASS|FAIL|SIG[A-Z]+|TIMEOUT|LEAK|FLAKY|TRY)[[:space:]]' "$1" \
    | sed -E 's/^[[:space:]]+//; s/\[[^]]*\]//; s/\([ 0-9]+\/[0-9]+\)//; s/[[:space:]]+/ /g' | sort -u > "$2"
}
fstype() { sed -n '1s/.*target\/tmp=\([^ ]*\) .*/\1/p' "$1"; }

for mode in base tmpfs; do
  echo "== $mode run"
  ROOT=$root BASE=$base RUN_UID=$(id -u) RUN_GID=$(id -g) unshare -r -m bash -c "$inner" tmpfs-run-inner "$mode" "$out/$mode.log" "${extra[@]}"
  rc=$?
  if ! grep -q '^nextest exit status' "$out/$mode.log" 2>/dev/null; then
    echo "the $mode run did not run the tests (rc $rc: no user/mount namespace, or a mount was refused)"; exit 2
  fi
  head -1 "$out/$mode.log"; tail -1 "$out/$mode.log"
  mklist "$out/$mode.log" "$out/$mode.list"
  [ -s "$out/$mode.list" ] || { echo "the $mode run produced no pass/fail list, see $out/$mode.log"; exit 2; }
  echo "$mode: $(grep -c '^PASS' "$out/$mode.list") pass, $(grep -vc '^PASS' "$out/$mode.list") not pass"
done

if [ "$(fstype "$out/base.log")" = "$(fstype "$out/tmpfs.log")" ]; then
  echo "both runs had the same scratch file system type ($(fstype "$out/base.log")): --base-dir must be on another one"; exit 2
fi
diff "$out/base.list" "$out/tmpfs.list" > "$out/list.diff"
if [ -s "$out/list.diff" ]; then
  { echo "DIFFERENT: the pass/fail lists differ (< base $(fstype "$out/base.log"), > tmpfs):"; head -40 "$out/list.diff"; } | tee "$out/summary.txt"
  exit 1
fi
echo "SAME: $(wc -l < "$out/base.list") tests, identical pass/fail on $(fstype "$out/base.log") and tmpfs (logs in $out)" | tee "$out/summary.txt"
