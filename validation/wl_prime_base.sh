#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Put the source of one commit on a host so wl_count.sh / wl_percall.sh can use it as the BASE from a role directory
# (which has no git): ~/wl/src/<sha> on the host. Run from a git checkout of the repository, on the machine that has the
# ops tools (VALIDATION-BASE-SHA). Nothing is built here; wl_prep.sh builds and caches the products on first use.
#
#   bash validation/wl_prime_base.sh <host> <sha-or-rev>
#
# Does nothing when ~/wl/src/<sha> or ~/wl/products/$(wl_key <sha>) (validation/wl_lock.sh) is already there. Prints "primed <host>:~/wl/src/<sha>" or
# "present ...". The sha it names is the full sha: pass that to --base, or read it from the role's .synced (merge_base=).
set -eu
[ $# -eq 2 ] || { sed -n '2,/^set -eu/p' "$0" | sed '$d'; exit 2; }
host=$1
sha=$(git rev-parse --verify "$2^{commit}")
. "${LIELLA_OPS:-$HOME/src/liella-ops}/tools/vm_ssh.sh"; vm_ssh_cmd "$host"
. "$(dirname "$0")/wl_lock.sh"   # wl_key: products are keyed by sha and products profile (WL-PREP-RELEASE-PROFILE)
if "${VSSH[@]}" "[ -f ~/wl/src/$sha/Cargo.toml ] || [ -f ~/wl/products/$(wl_key "$sha")/libc.a ]"; then echo "present $host:~/wl/src/$sha"; exit 0; fi
"${VSSH[@]}" "mkdir -p ~/wl/src && rm -rf ~/wl/src/$sha ~/wl/src/.$sha.p && mkdir ~/wl/src/.$sha.p"
set -o pipefail   # a failed git archive must not move an empty tree into the cache
git archive --format=tar "$sha" | "${VSSH[@]}" "tar -x -C ~/wl/src/.$sha.p && [ -f ~/wl/src/.$sha.p/Cargo.toml ] && mv -T ~/wl/src/.$sha.p ~/wl/src/$sha" || { "${VSSH[@]}" "rm -rf ~/wl/src/.$sha.p"; echo "wl_prime_base: could not unpack $sha" >&2; exit 1; }
echo "primed $host:~/wl/src/$sha"
