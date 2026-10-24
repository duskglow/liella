# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Sourced by wl_count.sh and wl_gate.sh: install the wl_* scripts on keke into a directory named by their content hash
# (TOOLS-SYNC 2, 2026-10-10). Every checkout used to overwrite ~/wl/bin; a run from a tree without the build lock replaced
# the locked wl_prep.sh and two builders of one sha ran at once. Now each content has its own dir, so nothing is replaced
# under a running job. Sets B (remote path, e.g. ~/wl/bin/v-<hash>). Needs $here and $keke.
wl_install_scripts() {
    local h inc
    h=$(cat "$here"/wl_* | md5sum | cut -c1-12); B="~/wl/bin/v-$h"
    $keke "mkdir -p ~/wl/bin && find ~/wl/bin -maxdepth 1 -name 'v-*' -mtime +2 -exec rm -rf {} + ; test -d $B" && return 0
    inc=.in.$$; $keke "rm -rf ~/wl/bin/$inc; mkdir -p ~/wl/bin/$inc" && scp -q -o BatchMode=yes "$here"/wl_* claude@10.0.100.53:wl/bin/$inc/ \
        && $keke "mv -T ~/wl/bin/$inc $B 2>/dev/null || rm -rf ~/wl/bin/$inc; test -d $B"
}
