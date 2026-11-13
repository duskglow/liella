#!/bin/bash
# LAYOUT-TRACE-HOST-CANONICAL (LT-4): the data files a hot-text trace reads from the tree, not the host.
# usage: tools/hottext/canon_host.sh <data dir>
#   Writes into <data dir> the files below and prints, on its last line, the redirections for
#   trace_gdb.py (HOT_TEXT_REDIRECT: `<host path>=<file>` joined by `:`). The tracer rewrites the path
#   argument of the traced program's open / stat / access calls of each host path to its file, at
#   system call entry: the program runs in the host's namespaces with its own IDs (no namespace: the
#   name-service cache keeps a private table in a user namespace, nscache.rs, and the trace would
#   miss its shared-region path) and reads:
#     /etc/passwd, /etc/group   one entry each for the caller's IDs (name `liella`), and root
#     /etc/nsswitch.conf        tools/hottext/hostdata/nsswitch.conf (a stock host's database lines)
#     /usr/lib/locale/locale-archive   an archive holding en_US.UTF-8 only (the probes' LANG)
#     /dev/shm/liella-nscache-<uid>    a fresh file per run of the program (`@run/`, trace_gdb.py;
#                               ruling #5223): every run creates the name-service cache region, whether
#                               or not the host's /dev/shm holds one (a gated job's /dev/shm is private)
#   The data model a stock host: an empty or bare stand-in runs other library paths than deployed hosts
#   run. What remains of the host: the archive is compiled by the host's `localedef` from its en_US
#   source; /usr/share/locale (locale.alias, catalogs: distribution data the tree does not carry), the
#   zone files and the kernel. The line before the last prints their digests, so a cross-host
#   difference of the traces can be traced to its input.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
D=$1
mkdir -p "$D/root/usr/lib/locale"
D=$(cd "$D" && pwd)
u=$(id -u) g=$(id -g)
printf 'root:x:0:0:root:/root:/bin/sh\nliella:x:%s:%s:Liella trace:/nonexistent:/bin/sh\n' "$u" "$g" > "$D/passwd"
printf 'root:x:0:\nliella:x:%s:\n' "$g" > "$D/group"
cp "$here/hostdata/nsswitch.conf" "$D/nsswitch.conf"
a=$D/root/usr/lib/locale/locale-archive
rc=0
localedef -c -i en_US -f UTF-8 --prefix="$D/root" en_US.UTF-8 > "$D/localedef.log" 2>&1 || rc=$?
if [ "$rc" -gt 1 ] || [ ! -s "$a" ]; then echo "canon_host: localedef en_US.UTF-8 failed (rc $rc): $(tail -2 "$D/localedef.log")" >&2; exit 1; fi
dg() { (sha256sum < "$1" 2>/dev/null || echo missing) | cut -c1-16; }
z=/usr/share/zoneinfo/America/New_York
echo "canon_host: host inputs left: kernel $(uname -r); $(localedef --version | head -1); archive $(dg "$a");" \
     "/usr/share/locale/locale.alias $(dg /usr/share/locale/locale.alias); $z $(dg $z)"
echo "/etc/passwd=$D/passwd:/etc/group=$D/group:/etc/nsswitch.conf=$D/nsswitch.conf:/usr/lib/locale/locale-archive=$a:/dev/shm/liella-nscache-$u=@run/nscache"
