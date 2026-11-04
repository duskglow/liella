# PERF-RC1024-STRLEN handoff (WIP, no code change yet)

State: branch task/perf-rc1024-strlen, base origin/next 125c0ccc. No product code changed. Status: NOT READY,
the bisect count is still in the queue on keke.

## What the RC-1024 A/B shows (liella-ops results/rc-1024, 76041c21 vs fa678f74)
- bash-loop +0.87% instr:u, `liella_machine_strlen` 1381M -> 1516M samples.
- make-n +0.54%: strlen 31M -> 52M, memmove 6M -> 18M, `__libc_calloc` 10M -> 21M. Because memmove and calloc grew
  too, the cause may be on the caller side (longer or more strlen inputs), not only scan.S.
- du +0.65%: this is NOT the strlen cause. Its growth is in `fts_build` (+12M) and `fts_alloc` (+3M), and strlen fell
  (4M -> 1M). Going by commit titles, the candidates are perf-fts-nochdir (5dd88211) and perf-fts-nochdir-2 (e45c4758).
  I did not count them. It needs its own story, or one more bisect pair.
- scan.S on origin/next is the same blob as at 76041c21 (6c161660), so next still carries whatever changed there.

## Candidates (first-parent merges fa678f74..76041c21 that touch csys/src/machine/scan.S)
5b915eec perf-strnlen-mid-3, c92cd5c7 perf-memrchr-nonavx2, ba069873 perf-strcasecmp-sse2-2,
e5e48aa2 perf-strcasecmp-sse2-long-4, fee0d7f1 perf-strcmp-loop. The chain counted is each merge and its first
parent (10 commits). Together with fa678f74 (base) and 76041c21 (the RC count), this puts any jump either on one
scan.S merge or in the stretch between two of them. Commit list: keke `~/work/perf-rc1024-strlen/shas.txt`.

## Jobs (keke)
- perf-rc1024-strlen-bisect: SUPERSEDED. rc 1, because wl_count refuses a directory with no `.synced`
  (`~/wl/src/<sha>` has none).
- perf-rc1024-strlen-bisect2: queued at 02:39Z, still queued at 02:49Z (load ~19-22). The script is
  `~/work/perf-rc1024-strlen/bisect2.sh`. It runs `WL_BASE_EXACT=1 wl_count.sh --base fa678f74 --rev <sha>
  ~/work/perf-rc1024-strlen/t/<sha> bash-loop make-n` for each commit. The `t/<sha>` stubs are role-like directories
  that hold only a `.synced` (head=<sha>); the sources were primed into `~/wl/src/<sha>` with wl_prime_base.sh.
  Per-commit logs go to `~/work/perf-rc1024-strlen/count-<sha>.log`.
  To read the results: `ssh claude@10.0.100.53 'grep -P "^perf-rc1024-strlen-bisect2\t" ~/logs/jobs.tsv; cat ~/logs/perf-rc1024-strlen-bisect2.log'`

## Next step
1. Read bisect2: find the first commit in the chain where bash-loop or make-n instr:u jumps.
2. If the jump is on a scan.S merge, compare the current strlen paths (scan.S lines 121-515: the AVX2 SMALL-STR head
   D-4.45, h64/h96, and the 128-byte loop) at the string lengths bash and make pass. Fix the strlen path, then check
   `cargo xtask bench --compare --counters strlen` (no strlen row worse) and wl_count bash-loop make-n against
   fa678f74. If the jump is in a stretch between merges, bisect that stretch the same way. It is then probably a
   caller-input change, which matches the memmove/calloc growth in make-n.
3. Gate (direct tier) and plants on the fix, as the brief says.

Do not redo: priming (all 10 sources are in keke ~/wl/src), the stub directories, or the du breakdown (above).
