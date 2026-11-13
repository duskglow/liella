#!/bin/sh
# tools/deb/proof.sh DEBDIR SUITE [LOGDIR]: prove the reference packages in an unprivileged chroot
# (no root: mmdebstrap --mode=unshare, the chroot discarded at the end). No spec clause: release
# packaging (records/pending/RELEASE-PACKAGING-DEB-1.toml).
#
# SUITE: trixie (Debian stable, https://deb.debian.org/debian) or excalibur (Devuan,
# http://deb.devuan.org/merged; Devuan's mirror network serves plain HTTP, apt checks the
# signatures with the keyring). Only those official mirrors are used.
#
# In the chroot (tools/deb/inside.sh): install the packages; build and run the smoke programs
# with liella-gcc/liella-g++ (dynamic and static); check that the system's programs still run and
# that libc6 and libc6-dev are unchanged (dpkg --verify); lintian the packages (when the suite has
# lintian); purge and check that nothing is left (the file list before install and after purge are
# equal). Prints inside.sh's "proof:" lines; exit 1 when any is FAIL.
#
# Environment: MMDEBSTRAP (default: mmdebstrap on PATH), KEYRING (default: the suite's keyring in
# /usr/share/keyrings), PROOF_LINTIAN=0 to skip lintian.
set -eu
[ $# -ge 2 ] || { echo "usage: tools/deb/proof.sh DEBDIR SUITE [LOGDIR]" >&2; exit 2; }
debdir=$(cd "$1" && pwd)
suite=$2
logdir=${3:-$debdir}
here=$(cd "$(dirname "$0")" && pwd)
mm=${MMDEBSTRAP:-mmdebstrap}
case $suite in
    trixie) mirror=https://deb.debian.org/debian; keyring=${KEYRING:-/usr/share/keyrings/debian-archive-keyring.gpg} ;;
    excalibur) mirror=http://deb.devuan.org/merged; keyring=${KEYRING:-/usr/share/keyrings/devuan-archive-keyring.gpg} ;;
    *) echo "proof.sh: suite $suite: trixie or excalibur" >&2; exit 2 ;;
esac
[ -e "$keyring" ] || { echo "proof.sh: no keyring $keyring (set KEYRING: a keyring file or a directory of them)" >&2; exit 2; }
command -v "$mm" > /dev/null || [ -x "$mm" ] || { echo "proof.sh: no mmdebstrap (set MMDEBSTRAP)" >&2; exit 2; }

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
# The namespace's root is a subordinate id: everything mmdebstrap reads from inside it (the
# keyring) must be readable by others, so it is copied here and the directory opened.
chmod 0755 "$stage"
mkdir -p "$stage/proof/debs" "$logdir"
if [ -d "$keyring" ]; then
    mkdir "$stage/keyrings"
    cp "$keyring"/*.gpg "$stage/keyrings/"
    keyring=$stage/keyrings
else
    cp "$keyring" "$stage/keyring.gpg"
    keyring=$stage/keyring.gpg
fi
cp "$debdir"/liella6*_amd64.deb "$stage/proof/debs/"
cp -r "$here/smoke" "$stage/proof/smoke"
cp "$here/inside.sh" "$stage/proof/inside.sh"
chmod -R a+rX "$stage"
log=$logdir/proof-$suite.log
lint=lintian
[ "${PROOF_LINTIAN:-1}" != 0 ] || lint=""

# The chroot gets a compiler, the system's C headers and readelf (binutils); lintian runs inside.
"$mm" --mode=unshare --variant=apt --keyring="$keyring" --components=main \
    --include="gcc,g++,libc6-dev,binutils,file${lint:+,$lint}" \
    --customize-hook="copy-in $stage/proof /tmp" \
    --customize-hook="chroot \"\$1\" sh /tmp/proof/inside.sh $suite" \
    "$suite" /dev/null "$mirror" > "$log" 2>&1 || echo "proof: FAIL mmdebstrap exited $? (see $log)" >> "$log"
grep '^proof:' "$log" || true
if grep -q '^proof: FAIL' "$log" || ! grep -q '^proof: RESULT' "$log"; then
    exit 1
fi
