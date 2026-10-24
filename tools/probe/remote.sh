#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# remote.sh <file.c> [probe options...]   - run ~/bin/probe on keke for one probe source.
#   tools/probe/remote.sh my_probe.c --liella ~/w/b --args a b
# The source (and the file named by --stdin, if any) is copied to ~/probe-in/ on keke, then
# `~/bin/probe` runs there; its output and exit status (0 SAME, 1 DIFFERENT, 2 error) come back.
# `remote.sh --install` copies tools/probe/probe to keke as ~/bin/probe.
# Other probe modes need no file: `remote.sh --find word...`, `--show HASH`, `--record HASH`, `--selftest`.
set -eu
KEKE=claude@10.0.100.53
SSH="ssh -o BatchMode=yes $KEKE"
here=$(cd "$(dirname "$0")" && pwd)
if [ "${1:-}" = "--install" ]; then
    scp -o BatchMode=yes "$here/probe" "$KEKE:bin/probe.new" >/dev/null
    $SSH 'chmod 755 ~/bin/probe.new && mv ~/bin/probe.new ~/bin/probe && mkdir -p ~/probe-in && echo installed ~/bin/probe'
    exit 0
fi
[ $# -ge 1 ] || { echo "usage: remote.sh <file.c> [probe options]  |  --install | --find W.. | --show H | --record H | --selftest" >&2; exit 2; }
$SSH 'mkdir -p ~/probe-in'
cmd='~/bin/probe'
file=
case "$1" in
    --*) ;;
    *) file=$1; shift
       [ -f "$file" ] || { echo "remote.sh: no such file: $file" >&2; exit 2; }
       h=$(sha256sum "$file" | cut -c1-12)
       rname="probe-in/$h-$(basename "$file")"
       scp -o BatchMode=yes "$file" "$KEKE:$rname" >/dev/null
       cmd="$cmd $(printf '%q' "$rname")" ;;
esac
while [ $# -gt 0 ]; do
    a=$1; shift
    if [ "$a" = "--stdin" ] && [ $# -gt 0 ]; then
        sf=$1; shift
        [ -f "$sf" ] || { echo "remote.sh: no such --stdin file: $sf" >&2; exit 2; }
        rs="probe-in/$(sha256sum "$sf" | cut -c1-12)-stdin"
        scp -o BatchMode=yes "$sf" "$KEKE:$rs" >/dev/null
        cmd="$cmd --stdin $(printf '%q' "$rs")"
    else
        case "$a" in
            '~/'*) cmd="$cmd ~/$(printf '%q' "${a#\~/}")" ;;   # a tree path such as ~/w/b is expanded on keke
            *) cmd="$cmd $(printf '%q' "$a")" ;;
        esac
    fi
done
exec $SSH "$cmd"
