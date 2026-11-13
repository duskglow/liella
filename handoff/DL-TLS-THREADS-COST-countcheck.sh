#!/bin/bash
# DL-TLS-THREADS-COST plant check: static pthread_create + pthread_join must stay at or below LIMIT
# instructions per call (wl_percall, this tree's products, built fresh: the rev is the hash of thread.rs).
# usage: bash handoff/DL-TLS-THREADS-COST-countcheck.sh <limit>   (run from the tree to measure)
set -u
limit=$1
rev=plant$(sha1sum crates/boundary/src/thread.rs | cut -c1-12)
out=$(COUNT_FRESH=1 bash validation/wl_percall.sh --base none --rev "$rev" --spec validation/percall/thread_create.c . 2>&1)
rc=$?
echo "$out" | grep -E 'per call|^case|thread_create_join'
[ $rc -eq 0 ] || { echo "COUNT-ERROR: wl_percall rc $rc"; exit 2; }
new=$(echo "$out" | awk '$1 == "case" { for (i = 1; i <= NF; i++) if ($i == "new") c = i } $1 == "thread_create_join" && c { print $c }')
[ -n "$new" ] || { echo "COUNT-ERROR: no thread_create_join row"; exit 2; }
if awk -v n="$new" -v l="$limit" 'BEGIN { exit !(n > l) }'; then
  echo "COUNT-OVER: thread_create_join new $new > limit $limit"
  exit 1
fi
echo "COUNT-OK: thread_create_join new $new <= limit $limit"
