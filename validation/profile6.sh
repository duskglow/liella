#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Timing and profiles of the milestone-6 packages (validation/profiles.md), glibc builds against
# Liella builds, the same way as validation/profile.sh for the first packages.
#
#   validation/profile6.sh OUT_DIR
#
# ONLY=REGEX limits the run to the matching workload names.
# Uses the suite builds in $VALIDATION/build/{glibc,liella-m}/{grep,gawk,diffutils,tar,gzip}
# and, for make, {glibc,liella}-noload (T27). Each workload runs under `perf stat -r 5` (one
# warm-up run first, discarded) for both builds, and once under `perf record` for each; each
# timing records the load before and after. perf runs through sudo -n.
set -u
V=${VALIDATION:-$HOME/validation}
out=$1
mkdir -p "$out"
work=$V/work
mkdir -p "$work"
B=$V/build

# Inputs: words.txt as in profile.sh (40 MB of words, numbers and paths), a copy with every
# 500th line changed (diff), its gzip, an archive of /usr/include (tar listing), and a makefile
# of 20,000 targets.
if [ ! -f "$work/words.txt" ]; then
    awk 'BEGIN { srand(42); for (i = 0; i < 4000000; i++) { w = ""; n = 3 + int(rand() * 8);
         for (j = 0; j < n; j++) w = w sprintf("%c", 97 + int(rand() * 26));
         printf "%s %d /usr/lib/%s/%d.so\n", w, int(rand() * 1000000), w, i } }' > "$work/words.txt"
fi
[ -f "$work/words2.txt" ] || awk 'NR % 500 == 0 { print "changed " NR; next } { print }' "$work/words.txt" > "$work/words2.txt"
[ -f "$work/words.txt.gz" ] || gzip -c "$work/words.txt" > "$work/words.txt.gz"
[ -f "$work/include.tar" ] || tar -cf "$work/include.tar" -C /usr include
if [ ! -f "$work/big.mk" ]; then
    {
        echo 'all: $(addprefix t,$(shell seq 1 20000))'
        echo 't%: ; @:'
        awk 'BEGIN { for (i = 1; i <= 20000; i++) printf "t%d: d%d\n", i, i % 977;
                     for (i = 0; i < 977; i++) printf "d%d: ;\n", i }'
    } > "$work/big.mk"
fi
nm "$V/products-m6/libc.a" 2>/dev/null | awk 'NF == 3 { print $3 }' | sort -u > "$work/liella-symbols"

# name|package|command (run from $work; @ is the program's build tree)
workloads=(
    # grep stops at the first match when its output is /dev/null: the counts go through cat.
    "grep-F|grep|sh -c 'LC_ALL=C @/src/grep -c usr/lib/q words.txt | cat'"
    "grep-E|grep|sh -c 'LC_ALL=C @/src/grep -cE \"^[a-f]+ [0-9]{5} \" words.txt | cat'"
    "grep-i-utf8|grep|sh -c 'LC_ALL=C.UTF-8 @/src/grep -ci qu[a-e] words.txt | cat'"
    "gawk-sum|gawk|env LC_ALL=C @/gawk '{ s += \$2 } END { print s }' words.txt"
    "gawk-regex|gawk|env LC_ALL=C @/gawk '/^[a-f]+ [0-9]+ / { n++ } END { print n }' words.txt"
    "gawk-utf8|gawk|env LC_ALL=C.UTF-8 @/gawk '{ n += length(\$1); a[substr(\$1, 1, 2)]++ } END { print n, length(a) }' words.txt"
    "gawk-printf|gawk|env LC_ALL=C @/gawk 'BEGIN { for (i = 0; i < 1000000; i++) printf \"%d %s %x %.3f\\n\", i, \"ab\", i, i / 7 }'"
    "diff|diffutils|env LC_ALL=C @/src/diff words.txt words2.txt"
    "tar-c|tar|sh -c '@/src/tar -cf - -C /usr include | cat > /dev/null'"
    "tar-tv|tar|env LC_ALL=C.UTF-8 @/src/tar -tvf include.tar"
    "gzip-c|gzip|@/gzip -c words.txt"
    "gzip-d|gzip|@/gzip -dc words.txt.gz"
    "make-n|make|@/make -n -f big.mk"
)
load() { uptime | sed 's/.*load average: //'; }
asme=(setpriv --reuid="$(id -u)" --regid="$(id -g)" --init-groups)
tree() { # BUILD PACKAGE
    local b=$1 p=$2
    if [ "$p" = make ]; then b=${b%-m}-noload; fi
    case $p in
        grep) echo "$B/$b/grep/grep-3.12" ;; gawk) echo "$B/$b/gawk/gawk-5.4.1" ;;
        diffutils) echo "$B/$b/diffutils/diffutils-3.12" ;; tar) echo "$B/$b/tar/tar-1.35" ;;
        gzip) echo "$B/$b/gzip/gzip-1.15" ;; make) echo "$B/$b/make/make-4.4.1" ;;
    esac
}
# CHECK=1: run each workload once per build and compare the outputs (no timing).
if [ "${CHECK:-0}" = 1 ]; then
    for w in "${workloads[@]}"; do
        name=${w%%|*} rest=${w#*|}
        pkg=${rest%%|*} cmd=${rest#*|}
        sums=
        for b in glibc liella-m; do
            c=${cmd//@/$(tree $b "$pkg")}
            s=$(cd "$work" && sh -c "$c" 2>&1 | md5sum | cut -c1-12)
            sums="$sums $b=$s"
        done
        echo "$name$sums"
    done
    exit 0
fi
results=$out/timings${ONLY:+-$ONLY}.tsv
printf 'workload\tbuild\tcpu_s\tstddev_pct\tload_before\tload_after\twall\n' > "$results"
for w in "${workloads[@]}"; do
    name=${w%%|*}
    # ONLY=REGEX: just the workloads whose name matches.
    [[ $name =~ ${ONLY:-.} ]] || continue
    rest=${w#*|}
    pkg=${rest%%|*}
    cmd=${rest#*|}
    for build in glibc liella; do
        b=glibc
        [ $build = liella ] && b=liella-m
        c=${cmd//@/$(tree $b "$pkg")}
        (cd "$work" && sh -c "$c" > /dev/null 2>&1) # warm-up, discarded
        before=$(load)
        stat=$(cd "$work" && sudo -n perf stat -r 5 -x, -e task-clock "${asme[@]}" sh -c "$c" 2>&1 > /dev/null | tail -1)
        wall=$(cd "$work" && { /usr/bin/time -f %e sh -c "$c" > /dev/null; } 2>&1 | tail -1)
        after=$(load)
        ms=$(echo "$stat" | cut -d, -f1)
        sd=$(echo "$stat" | cut -d, -f4)
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$build" "$(echo "scale=3; $ms / 1000" | bc)" "$sd" "$before" "$after" "$wall" >> "$results"
        data=$out/$name.data
        [ $build = glibc ] && data=$out/$name-glibc.data
        (cd "$work" && sudo -n perf record -q -F 2000 -o "$data" "${asme[@]}" sh -c "$c" > /dev/null 2>&1)
        sudo -n chown "$(id -u):$(id -g)" "$data"
    done
    perf report -q -i "$out/$name.data" --no-children --sort symbol --stdio 2>/dev/null |
        grep -E '^ +[0-9]' | head -25 > "$out/$name.top"
    perf report -q -i "$out/$name-glibc.data" --no-children --sort dso,symbol --stdio 2>/dev/null |
        grep -E '^ +[0-9]' | head -25 > "$out/$name-glibc.top"
    rm -f "$out/$name.data" "$out/$name-glibc.data"
done
cat "$results"
