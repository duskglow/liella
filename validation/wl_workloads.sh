# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Workload definitions of the workload gate (validation/wl_gate.sh), sourced by wl_run.sh.
# WL_DEFS lines: name|command. In a command, @ is the kit's bin directory; the command runs
# from the work directory ($WL/work) and writes its result to standard output (the runner
# sends it to /dev/null for timing and to md5sum for the output check). Inputs are generated
# once by wl_prep.sh and shipped to every host, so every host and every variant sees the same
# bytes. tree = /usr/include and /usr/share/man of keke. en_US.UTF-8 rows exercise rule-based collation (strcoll); the locale is in the hosts' locale archive.
WL_DEFS=(
    "sort-C|env LC_ALL=C @/sort words.txt"
    "sort-utf8|env LC_ALL=C.UTF-8 @/sort -k1,1 words.txt"
    "sort-enUS|env LC_ALL=en_US.UTF-8 @/sort -k1,1 words.txt"
    "ls-l-enUS|sh -c 'for i in 1 2 3 4 5 6 7 8 9 10; do LC_ALL=en_US.UTF-8 @/ls -lR tree; done'"
    "sort-n|env LC_ALL=C @/sort -n -k2,2 words.txt"
    "wc|env LC_ALL=C.UTF-8 @/wc -lwm words.txt"
    "md5sum|@/md5sum words.txt"
    "cut-tr-uniq|sh -c '@/cut -d\" \" -f1 words.txt | @/tr a-z A-Z | @/uniq -c'"
    "seq|@/seq 1 30000000"
    "printf-loop|sh -c 'i=0; while [ \$i -lt 20000 ]; do @/printf \"%s %d %x\n\" a \$i \$i; i=\$((i+1)); done'"
    "ls-lR|sh -c 'for i in 1 2 3 4 5 6 7 8 9 10; do LC_ALL=C.UTF-8 @/ls -lR tree; done'"
    "du|sh -c 'for i in 1 2 3 4 5 6 7 8 9 10; do @/du -s tree; done'"
    "sed-s|env LC_ALL=C @/sed -E 's/([a-z]+) ([0-9]+)/\2:\1/; s/\.so\$/.a/' words.txt"
    "sed-utf8|env LC_ALL=C.UTF-8 @/sed 's/[[:alpha:]]*a[[:alpha:]]*/<&>/g' words.txt"
    "bash-loop|@/bash loop.sh"
    "grep-F|sh -c 'LC_ALL=C @/grep -c usr/lib/q words.txt | cat'"
    "grep-E|sh -c 'LC_ALL=C @/grep -cE \"^[a-f]+ [0-9]{5} \" words.txt | cat'"
    "grep-i-utf8|sh -c 'LC_ALL=C.UTF-8 @/grep -ci qu[a-e] words.txt | cat'"
    "gawk-sum|env LC_ALL=C @/gawk '{ s += \$2 } END { print s }' words.txt"
    "gawk-regex|env LC_ALL=C @/gawk '/^[a-f]+ [0-9]+ / { n++ } END { print n }' words.txt"
    "gawk-utf8|env LC_ALL=C.UTF-8 @/gawk '{ n += length(\$1); a[substr(\$1, 1, 2)]++ } END { print n, length(a) }' words.txt"
    "gawk-printf|env LC_ALL=C @/gawk 'BEGIN { for (i = 0; i < 1000000; i++) printf \"%d %s %x %.3f\n\", i, \"ab\", i, i / 7 }'"
    "diff|env LC_ALL=C @/diff words.txt words2.txt"
    "tar-c|sh -c '@/tar -cf - -C tree . | cat'"
    "tar-tv|env LC_ALL=C.UTF-8 @/tar -tvf tree.tar"
    "gzip-c|@/gzip -c words.txt"
    "gzip-d|@/gzip -dc words.txt.gz"
    "make-n|@/make -n -f big.mk"
    "pipe-mix|sh -c '@/grep -F -e ab -e cd words.txt | @/sort | @/uniq -c | @/gzip -c'"
    "tar-gz|sh -c '@/tar -cf - -C tree . | @/gzip -c'"
)
