#!/bin/sh
# STARTUP-DATA-PAGE-ALIGN proof (handoff/STARTUP-DATA-PAGE-ALIGN.md): the STARTUP-DATA-WINDOWS counts and page lists of
# a branch of origin and of the same branch with an unrelated branch merged, fast and release products, built in a
# scratch clone of origin (no tree is copied between hosts).
#
#   sh handoff/STARTUP-DATA-PAGE-ALIGN-proof.sh <dir> <tag> <branch> <other-branch> [faults]
#
# <dir>: a new RAM directory; the clone is <dir>/<tag>. <branch> is checked out and built (fast, then release), then
# <other-branch> is merged into it (git merge --no-edit, nothing pushed) and built again, incrementally. Each build
# prints `proof: <tag> <tree> <profile>: ...` lines: the `cargo xtask so` status, the contract's summary, the library
# sizes, and every written page and read-only window (layout-check again with the contract's budgets at 0, in the
# scratch clone only). `faults` also runs the start-up page-fault bound tests on each build.
set -eu
dir=$1; tag=$2; branch=$3; other=$4; faults=${5:-}
url=https://github.com/duskglow/liella.git
. "$HOME/.cargo/env"
mkdir -p "$dir"
t=$dir/$tag
git clone -q --branch "$branch" "$url" "$t"
cd "$t"
git fetch -q origin "$other"
echo "proof: $tag: $branch $(git rev-parse --short=12 HEAD), $other $(git rev-parse --short=12 FETCH_HEAD)"

build() {
    tree=$1
    for p in fast release; do
        if [ "$p" = release ]; then LIELLA_FULL_PROFILE=1; export LIELLA_FULL_PROFILE; else unset LIELLA_FULL_PROFILE; fi
        rc=0; cargo xtask so || rc=$?
        echo "proof: $tag $tree $p: so rc=$rc (rc 1 with only STARTUP-DATA-WINDOWS broken is an over-budget count)"
        echo "proof: $tag $tree $p: sizes $(stat -c '%n=%s' target/liella/libc.a target/liella/libc.so.6 | tr '\n' ' ')"
        cp layout/contracts.tsv "$dir/$tag.contracts.tsv"
        sed -E '/^STARTUP-DATA-WINDOWS\t/s/(r[ow]-budget-[a-z]+)=[0-9]+/\1=0/g' "$dir/$tag.contracts.tsv" > layout/contracts.tsv
        cargo xtask layout-check 2>&1 | grep -E 'STARTUP-DATA-WINDOWS(:| ok)' | grep -v -E 'BROKEN|protects' | sed "s/^/proof: $tag $tree $p: /" || true
        cat "$dir/$tag.contracts.tsv" > layout/contracts.tsv
        if [ "$faults" = faults ]; then
            frc=0; cargo test -q -p liella --test hot_text_products page_fault_bound || frc=$?
            echo "proof: $tag $tree $p: start-up page-fault bound tests rc=$frc"
        fi
    done
}

build "$branch"
git -c user.name=proof -c user.email=proof@invalid merge -q --no-edit FETCH_HEAD
echo "proof: $tag: merged $(git rev-parse --short=12 HEAD)"
build "$branch+$other"
git diff --quiet HEAD -- layout/contracts.tsv
echo "proof: $tag: done"
