#!/bin/sh
# Second-pass secret scan with gitleaks (release checklist gate 3b). Never prints a secret value (--redact, and the
# report is reduced to rule, path, short sha, line). Usage: tools/gitleaks_scan.sh <repo-dir> <out-dir>
# Installs the pinned official release into ~/bin (claude user only; no root) after verifying its sha256 against the
# release's checksums file. Output: <out-dir>/history.tsv and <out-dir>/tree.tsv (rule, path, sha, line).
set -eu
VER=8.30.1
REPO=${1:?repo dir}
OUT=${2:?out dir}
BIN=${HOME}/bin/gitleaks
mkdir -p "$OUT" "$HOME/bin"
if ! "$BIN" version 2>/dev/null | grep -q "$VER"; then
  T=$(mktemp -d)
  base=https://github.com/gitleaks/gitleaks/releases/download/v$VER
  tar_name=gitleaks_${VER}_linux_x64.tar.gz
  curl -fsSL -o "$T/$tar_name" "$base/$tar_name"
  curl -fsSL -o "$T/checksums.txt" "$base/gitleaks_${VER}_checksums.txt"
  (cd "$T" && grep " $tar_name\$" checksums.txt | sha256sum -c -)
  mkdir "$T/x" && tar -xzf "$T/$tar_name" -C "$T/x" gitleaks
  install -m 755 "$T/x/gitleaks" "$BIN"
fi
"$BIN" version
cd "$REPO"
CFG=""
[ -n "${GITLEAKS_CONFIG:-}" ] && CFG="--config $GITLEAKS_CONFIG"
# full history (all refs) and the current tree (tracked + untracked, files on disk)
"$BIN" git $CFG --redact --no-banner --report-format json --report-path "$OUT/history.json" --exit-code 0 . </dev/null || true
"$BIN" dir $CFG --redact --no-banner --report-format json --report-path "$OUT/tree.json" --exit-code 0 . </dev/null || true
for k in history tree; do
  python3 -I - "$OUT/$k.json" > "$OUT/$k.tsv" <<'PY'
import json, sys
for f in json.load(open(sys.argv[1])) or []:
    print(f.get("RuleID"), f.get("File"), (f.get("Commit") or "worktree")[:8], f.get("StartLine"), sep="\t")
PY
done
wc -l "$OUT/history.tsv" "$OUT/tree.tsv"
