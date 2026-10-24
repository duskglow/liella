#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Download the milestone-6 sources and check their signatures (validation/README.md, Method).
#
#   validation/fetch.sh
#
# Exactly the six tarballs the owner approved (2026-10-05) and their detached signatures, from
# https://ftp.gnu.org/gnu/ into $VALIDATION (default ~/validation). Each signature is checked
# with gpgv against gnu-keyring.gpg (already there); a size that differs from the approved one,
# or a bad signature, stops the script with a non-zero status before anything is unpacked.
# Prints, per tarball: size, SHA-256 and the signer gpgv names.
set -eu
V=${VALIDATION:-$HOME/validation}
cd "$V"
files=(
    "grep/grep-3.12.tar.xz 1918448"
    "gawk/gawk-5.4.1.tar.xz 3838416"
    "diffutils/diffutils-3.12.tar.xz 1938800"
    "tar/tar-1.35.tar.xz 2317208"
    "gzip/gzip-1.15.tar.xz 808900"
    "make/make-4.4.1.tar.gz 2348200"
)
for f in "${files[@]}"; do
    path=${f% *}
    size=${f#* }
    name=${path#*/}
    for x in "$name" "$name.sig"; do
        [ -s "$x" ] || curl -fsS --proto =https -o "$x" "https://ftp.gnu.org/gnu/${path%/*}/$x"
    done
    got=$(stat -c %s "$name")
    if [ "$got" != "$size" ]; then
        echo "$name: size $got, approved $size: stopping" >&2
        exit 1
    fi
    if ! sig=$(gpgv --keyring "$V/gnu-keyring.gpg" "$name.sig" "$name" 2>&1); then
        echo "$name: BAD SIGNATURE: stopping" >&2
        echo "$sig" >&2
        exit 1
    fi
    signer=$(echo "$sig" | sed -n 's/.*Good signature from "\(.*\)".*/\1/p' | head -1)
    echo "$name size=$got sha256=$(sha256sum "$name" | cut -d' ' -f1) signer=$signer"
done
