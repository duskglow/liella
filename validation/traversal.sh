#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# A directory removed during a traversal (D-13.13; spec §13.2, readdir on a removed directory):
# how ls -R, find, du and rm -r behave, built against glibc and against Liella.
#
#   validation/traversal.sh GLIBC_BIN_DIR LIELLA_BIN_DIR OUT_DIR
#
# Each BIN_DIR holds ls, du, rm and find from the validation builds. The tree is
# top/{a,b,c}, with b holding three files and a subdirectory. The moment is made exact with
# strace's syscall delay: a first traced run lists the getdents64 calls with the descriptor's
# path (-y) and finds the call on top/b; a second run delays that call by 1.5 s at entry, during
# which top/b is removed (rm -rf by the host's rm), so the call meets a removed directory. Two
# moments: the first read of top/b (before any of its entries were seen), and the read after the
# first (its entries seen, the end of the directory not yet). The output records each program's
# exit status, standard error (its lines sorted, so the readdir order of the scratch file system
# does not show) and how many lines it printed.
set -u
G=$1
L=$2
out=$3
mkdir -p "$out"
work=$(mktemp -d "$out/work.XXXXXX")
make_tree() {
    rm -rf "$work/top"
    mkdir -p "$work/top/a" "$work/top/b/sub" "$work/top/c"
    touch "$work/top/a/f1" "$work/top/a/f2" "$work/top/b/f1" "$work/top/b/f2" "$work/top/b/f3" \
        "$work/top/b/sub/g1" "$work/top/c/f1"
}
run_case() { # libc bindir tool moment
    local libc=$1 bin=$2 tool=$3 moment=$4 cmd
    case $tool in
        ls) cmd=("$bin/ls" -R top) ;;
        find) cmd=("$bin/find" top) ;;
        du) cmd=("$bin/du" top) ;;
        rm) cmd=("$bin/rm" -r top) ;;
    esac
    make_tree
    # The getdents64 calls, in order, with their descriptors' paths.
    (cd "$work" && strace -qq -e trace=getdents64 -y -o "$work/trace" "${cmd[@]}" > /dev/null 2>&1)
    local n
    n=$(grep -n "getdents64([0-9]*</[^>]*/top/b>" "$work/trace" | sed -n "${moment}p" | cut -d: -f1)
    make_tree
    if [ -z "$n" ]; then
        echo "$libc	$tool	read $moment of top/b	(no such getdents64 call)"
        return
    fi
    (cd "$work" && strace -qq -o /dev/null -e trace=getdents64 -e inject=getdents64:delay_enter=1500000:when="$n" \
        "${cmd[@]}" > "$work/stdout" 2> "$work/stderr") &
    local pid=$!
    sleep 0.7
    rm -rf "$work/top/b"
    wait $pid
    local rc=$?
    printf '%s\t%s\tread %s of top/b\texit %s\t%s lines\t%s\n' "$libc" "$tool" "$moment" "$rc" \
        "$(wc -l < "$work/stdout")" "$(sed "s|$work/||g" "$work/stderr" | LC_ALL=C sort | tr '\n' '|')"
}
{
    for tool in ls find du rm; do
        for moment in 1 2; do
            run_case glibc "$G" $tool $moment
            run_case liella "$L" $tool $moment
        done
    done
} | tee "$out/traversal.tsv"
rm -rf "$work"
