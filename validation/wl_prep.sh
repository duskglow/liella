#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Workload gate, step 1 (runs on keke): products, link kits and inputs.   validation/wl_gate.sh
#
#   wl_prep.sh SHA...
#
# SHA: full commit ids whose source tree is already unpacked in ~/wl/src/SHA (wl_gate.sh does
# that with git archive). For each SHA: build the products (cargo xtask so; libc.a and
# liella-crt1.o) into ~/wl/products/SHA, then relink every workload program against them into
# the kit ~/wl/kit/SHA/bin. The program objects come from the validation builds
# (~/validation/build/liella-m, make: liella-noload), compiled with the system's headers, so
# they do not depend on the Liella commit; only the final link does. The link commands are
# taken once from `make -n` (nothing is built in the validation trees). The glibc kit
# ~/wl/kit/glibc/bin holds the dynamically linked host builds. Inputs: ~/wl/work.
set -eu
V=$HOME/validation
W=$HOME/wl
mkdir -p "$W/products" "$W/kit" "$W/recipes" "$W/work" "$W/src"
. "$(dirname "$0")/wl_lock.sh"

# name|path in the tree|main object|tree under build/<variant>/
PROGS=(
    "sort|src/sort|src/sort.o|coreutils/coreutils-9.12" "wc|src/wc|src/wc.o|coreutils/coreutils-9.12"
    "md5sum|src/md5sum|src/md5sum.o|coreutils/coreutils-9.12" "cut|src/cut|src/cut.o|coreutils/coreutils-9.12"
    "tr|src/tr|src/tr.o|coreutils/coreutils-9.12" "uniq|src/uniq|src/uniq.o|coreutils/coreutils-9.12"
    "seq|src/seq|src/seq.o|coreutils/coreutils-9.12" "printf|src/printf|src/printf.o|coreutils/coreutils-9.12"
    "ls|src/ls|src/ls.o|coreutils/coreutils-9.12" "du|src/du|src/du.o|coreutils/coreutils-9.12"
    "sed|sed/sed|sed/sed.o|sed/sed-4.10" "bash|bash|shell.o|bash/bash-5.3"
    "grep|src/grep|src/grep.o|grep/grep-3.12" "gawk|gawk|main.o|gawk/gawk-5.4.1"
    "diff|src/diff|src/diff.o|diffutils/diffutils-3.12" "tar|src/tar|src/tar.o|tar/tar-1.35"
    "gzip|gzip|gzip.o|gzip/gzip-1.15" "make|make|main.o|make/make-4.4.1"
)
# WL_PROGS="seq du" limits links (and the glibc copies) to those programs (wl_count.sh); default: all.
if [ -n "${WL_PROGS:-}" ]; then
    keep=()
    for p in "${PROGS[@]}"; do case " $WL_PROGS " in *" ${p%%|*} "*) keep+=("$p") ;; esac; done
    PROGS=("${keep[@]}")
fi
ltree() { # name tree -> the liella validation tree
    case $1 in make) echo "$V/build/liella-noload/$2" ;; *) echo "$V/build/liella-m/$2" ;; esac
}
gtree() { case $1 in make) echo "$V/build/glibc-noload/$2" ;; *) echo "$V/build/glibc/$2" ;; esac; }

# Link recipes (once).
for p in "${PROGS[@]}"; do
    IFS='|' read -r name path obj tree <<< "$p"
    f=$W/recipes/$name.cmd
    [ -s "$f" ] && continue
    t=$(ltree "$name" "$tree")
    # A recursive package (grep, diffutils, tar, coreutils is flat) keeps the rule in src/Makefile.
    sub= cmd=
    for sub in . "$(dirname "$path")"; do
        o=$obj; p=$path
        [ "$sub" = . ] || { o=$(basename "$obj") p=$(basename "$path"); }
        cmd=$(cd "$t/$sub" 2>/dev/null && make -n -W "$o" "$p" 2>/dev/null | grep -E -- " -o $p( |$)" | grep -v -- ' -c ' | tail -1 || true)
        [ -n "$cmd" ] && break
    done
    [ -n "$cmd" ] || { echo "wl_prep: no link command for $name" >&2; exit 1; }
    printf '%s\n%s\n%s\n' "$sub" "$p" "$cmd" > "$f.$$" && mv -f "$f.$$" "$f"
done

# glibc kit.
# Replaced by rename, never rewritten in place (another run may be executing the old copy).
mkdir -p "$W/kit/glibc/bin"
for p in "${PROGS[@]}"; do
    IFS='|' read -r name path obj tree <<< "$p"
    g=$(gtree "$name" "$tree")/$path
    cmp -s "$g" "$W/kit/glibc/bin/$name" || { cp -f "$g" "$W/kit/glibc/bin/.$name.$$" && mv -f "$W/kit/glibc/bin/.$name.$$" "$W/kit/glibc/bin/$name"; }
done

for sha in "$@"; do
    prod=$W/products/$sha
    wl_lock "$sha"
    wl_build_products "$sha" || exit 1
    kit=$W/kit/$sha/bin
    mkdir -p "$kit"
    for p in "${PROGS[@]}"; do
        IFS='|' read -r name path obj tree <<< "$p"
        [ -x "$kit/$name" ] && continue
        { read -r sub; read -r p; read -r cmd; } < "$W/recipes/$name.cmd"
        extra=
        # gawk's liella-m objects were configured without -lm (no libm.a then); its maths
        # functions come from Liella's libm.a, where -lm would put it (no stand-in since GAWK-LM-CHECK-2).
        [ "$name" = gawk ] && extra=$prod/libm.a
        (cd "$(ltree "$name" "$tree")/$sub" && LIELLA_PRODUCTS=$prod LIELLA_CC_EXTRA=$extra bash -c "${cmd/ -o $p / -o $kit/$name.tmp }") > "$W/kit/$sha/link-$name.log" 2>&1 || { echo "wl_prep: link of $name failed (see $W/kit/$sha/link-$name.log)" >&2; exit 1; }
        mv -f "$kit/$name.tmp" "$kit/$name"
    done
    wl_unlock
done

# Inputs (deterministic; shipped to the timing hosts with rsync). One run creates them at a time.
wl_lock inputs
w=$W/work
if [ ! -f "$w/words.txt" ]; then
    awk 'BEGIN { srand(42); for (i = 0; i < 1000000; i++) { w = ""; n = 3 + int(rand() * 8);
         for (j = 0; j < n; j++) w = w sprintf("%c", 97 + int(rand() * 26));
         printf "%s %d /usr/lib/%s/%d.so\n", w, int(rand() * 1000000), w, i } }' > "$w/words.txt"
fi
[ -f "$w/words2.txt" ] || awk 'NR % 500 == 0 { print "changed " NR; next } { print }' "$w/words.txt" > "$w/words2.txt"
[ -f "$w/words.txt.gz" ] || gzip -c "$w/words.txt" > "$w/words.txt.gz"
[ -f "$w/tree.tar" ] || tar -cf "$w/tree.tar" -C /usr include share/man
if [ ! -f "$w/big.mk" ]; then
    { echo 'all: $(addprefix t,$(shell seq 1 20000))'; echo 't%: ; @:'
      awk 'BEGIN { for (i = 1; i <= 20000; i++) printf "t%d: d%d\n", i, i % 977; for (i = 0; i < 977; i++) printf "d%d: ;\n", i }'
    } > "$w/big.mk"
fi
cat > "$w/loop.sh" <<'LOOP'
s=; n=0
for ((i = 0; i < 300000; i++)); do
    s="${s:0:20}x$i"; n=$(( (n + i * 7) % 1000003 ))
    case $s in *x1*) n=$((n + 1)) ;; esac
done
printf '%s %d\n' "${#s}" "$n"
LOOP
echo "wl_prep: done ($*)"
