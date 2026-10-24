#!/bin/sh
# The secret scan at the cut (release checklist gate 3 + 3b): one command, both passes, exit 0 only when both are clean
# after tools/secret_scan_allow.toml. Usage: sh tools/secret_scan_cut.sh [repo-dir] [out-dir]
# Needs network on first run (gitleaks install, see gitleaks_scan.sh) unless ~/bin/gitleaks 8.30.1 exists. Stdin is
# always /dev/null. Prints rule/path/commit/line only, never matched text.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "${1:-.}" && pwd)
OUT=${2:-$(mktemp -d)}
mkdir -p "$OUT"
rc=0
exec </dev/null
python3 -I "$HERE/secret_scan.py" "$REPO" > "$OUT/grep.tsv" || rc=1
echo "pass 1 (secret_scan.py): $(wc -l < "$OUT/grep.tsv") hits"
GITLEAKS_CONFIG="$HERE/secret_scan_allow.toml" sh "$HERE/gitleaks_scan.sh" "$REPO" "$OUT" > "$OUT/gitleaks.log" 2>&1 || rc=1
echo "pass 2 (gitleaks): history $(wc -l < "$OUT/history.tsv") hits, tree $(wc -l < "$OUT/tree.tsv") hits"
[ -s "$OUT/history.tsv" ] || [ -s "$OUT/tree.tsv" ] && rc=1
echo "outputs in $OUT; exit $rc"
exit $rc
