#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Timing and profiles of realistic workloads, glibc builds against Liella builds (validation
# step 5, input for the optimization thread; validation/profiles.md).
#
#   validation/profile.sh OUT_DIR
#
# Uses the validation builds in $VALIDATION/build/{glibc,liella-w}/{coreutils,sed,bash}. Each
# workload runs under `perf stat -r 5` (one warm-up run first, discarded) for both builds, and
# once under `perf record` for each; the report lists the hottest symbols and how
# much of the time is Liella's (symbols defined in libc.a) or, for glibc, libc.so.6's. Each
# timing records the machine's load (uptime) before and after. perf runs through sudo -n (the
# host's perf_event_paranoid is left as it is).
set -u
V=${VALIDATION:-$HOME/validation}
out=$1
mkdir -p "$out"
work=$V/work
mkdir -p "$work"
G=$V/build/glibc
L=$V/build/liella-w
cu=coreutils/coreutils-9.12/src
se=sed/sed-4.10/sed/sed
ba=bash/bash-5.3/bash

# Inputs: 40 MB of words, numbers and paths (deterministic).
if [ ! -f "$work/words.txt" ]; then
    awk 'BEGIN { srand(42); for (i = 0; i < 4000000; i++) { w = ""; n = 3 + int(rand() * 8);
         for (j = 0; j < n; j++) w = w sprintf("%c", 97 + int(rand() * 26));
         printf "%s %d /usr/lib/%s/%d.so\n", w, int(rand() * 1000000), w, i } }' > "$work/words.txt"
fi
cat > "$work/loop.sh" <<'EOF'
s=; n=0
for ((i = 0; i < 300000; i++)); do
    s="${s:0:20}x$i"; n=$(( (n + i * 7) % 1000003 ))
    case $s in *x1*) n=$((n + 1)) ;; esac
done
printf '%s %d\n' "${#s}" "$n"
EOF
nm "$V/products/libc.a" 2>/dev/null | awk 'NF == 3 { print $3 }' | sort -u > "$work/liella-symbols"

# name|command (run from $work; @ is the build directory)
workloads=(
    "sort-C|env LC_ALL=C @/$cu/sort -o /dev/null words.txt"
    "sort-utf8|env LC_ALL=C.UTF-8 @/$cu/sort -k1,1 -o /dev/null words.txt"
    "sort-n|env LC_ALL=C @/$cu/sort -n -k2,2 -o /dev/null words.txt"
    "wc|env LC_ALL=C.UTF-8 @/$cu/wc -lwm words.txt"
    "md5sum|@/$cu/md5sum words.txt"
    "cut-tr-uniq|sh -c '@/$cu/cut -d\" \" -f1 words.txt | @/$cu/tr a-z A-Z | @/$cu/uniq -c > /dev/null'"
    "seq|@/$cu/seq 1 3000000 > /dev/null"
    "printf-loop|sh -c 'i=0; while [ \$i -lt 20000 ]; do @/$cu/printf \"%s %d %x\\n\" a \$i \$i; i=\$((i+1)); done > /dev/null'"
    "ls-lR|env LC_ALL=C.UTF-8 @/$cu/ls -lR /usr/include /usr/share/man > /dev/null"
    "du|@/$cu/du -s /usr/share /usr/lib > /dev/null"
    "sed-s|env LC_ALL=C @/$se -E 's/([a-z]+) ([0-9]+)/\\2:\\1/; s/\\.so\$/.a/' words.txt > /dev/null"
    "sed-utf8|env LC_ALL=C.UTF-8 @/$se 's/[[:alpha:]]*a[[:alpha:]]*/<&>/g' words.txt > /dev/null"
    "bash-loop|@/$ba loop.sh"
)
load() { uptime | sed 's/.*load average: //'; }
# perf needs root here; the workload itself runs as the calling user.
asme=(setpriv --reuid="$(id -u)" --regid="$(id -g)" --init-groups)
results=$out/timings.tsv
printf 'workload\tbuild\tmean_s\tstddev_pct\tload_before\tload_after\n' > "$results"
for w in "${workloads[@]}"; do
    name=${w%%|*}
    cmd=${w#*|}
    for build in glibc liella; do
        dir=$G
        [ $build = liella ] && dir=$L
        c=${cmd//@/$dir}
        (cd "$work" && sh -c "$c" > /dev/null 2>&1) # warm-up, discarded
        before=$(load)
        stat=$(cd "$work" && sudo -n perf stat -r 5 -x, -e task-clock "${asme[@]}" sh -c "$c" 2>&1 > /dev/null | tail -1)
        wall=$(cd "$work" && { /usr/bin/time -f %e sh -c "$c" > /dev/null; } 2>&1 | tail -1)
        after=$(load)
        # perf stat -x: value,unit,event,stddev%,...: task-clock in ms (CPU time of the run).
        ms=$(echo "$stat" | cut -d, -f1)
        sd=$(echo "$stat" | cut -d, -f4)
        printf '%s\t%s\t%s\t%s\t%s\t%s\twall=%s\n' "$name" "$build" "$(echo "scale=3; $ms / 1000" | bc)" "$sd" "$before" "$after" "$wall" >> "$results"
    done
    # Profile of the Liella build.
    c=${cmd//@/$L}
    (cd "$work" && sudo -n perf record -q -F 2000 -o "$out/$name.data" "${asme[@]}" sh -c "$c" > /dev/null 2>&1)
    sudo -n chown "$(id -u):$(id -g)" "$out/$name.data"
    perf report -q -i "$out/$name.data" --no-children --sort symbol --stdio 2>/dev/null |
        grep -E '^ +[0-9]' | head -25 > "$out/$name.top"
    c=${cmd//@/$G}
    (cd "$work" && sudo -n perf record -q -F 2000 -o "$out/$name-glibc.data" "${asme[@]}" sh -c "$c" > /dev/null 2>&1)
    sudo -n chown "$(id -u):$(id -g)" "$out/$name-glibc.data"
    perf report -q -i "$out/$name-glibc.data" --no-children --sort dso,symbol --stdio 2>/dev/null |
        grep -E '^ +[0-9]' | head -25 > "$out/$name-glibc.top"
done
cat "$results"
