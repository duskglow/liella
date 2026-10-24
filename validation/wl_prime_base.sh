#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Put the source of one commit on a host so wl_count.sh / wl_percall.sh can use it as the BASE from a role directory
# (which has no git): ~/wl/src/<sha> on the host. Run from a git checkout of the repository, on the machine that has the
# ops tools (VALIDATION-BASE-SHA). Nothing is built here; wl_prep.sh builds and caches the products on first use.
#
#   bash validation/wl_prime_base.sh <host> <sha-or-rev>
#
# Does nothing when ~/wl/src/<sha> or ~/wl/products/<sha> is already there. Prints "primed <host>:~/wl/src/<sha>" or
# "present ...". The sha it names is the full sha: pass that to --base, or read it from the role's .synced (merge_base=).
set -eu
[ $# -eq 2 ] || { sed -n '2,/^set -eu/p' "$0" | sed '$d'; exit 2; }
host=$1
sha=$(git rev-parse --verify "$2^{commit}")
. "${LIELLA_OPS:-$HOME/src/liella-ops}/tools/vm_ssh.sh"; vm_ssh_cmd "$host"
if "${VSSH[@]}" "[ -d ~/wl/src/$sha ] || [ -f ~/wl/products/$sha/libc.a ]"; then echo "present $host:~/wl/src/$sha"; exit 0; fi
"${VSSH[@]}" "mkdir -p ~/wl/src && rm -rf ~/wl/src/.$sha.p && mkdir ~/wl/src/.$sha.p"
git archive --format=tar "$sha" | "${VSSH[@]}" "tar -x -C ~/wl/src/.$sha.p && mv -T ~/wl/src/.$sha.p ~/wl/src/$sha"
echo "primed $host:~/wl/src/$sha"
