# TIMING-COVERAGE-1106B: the remaining uncovered slower-in-time rows of the 1106 table, briefed

First line: COMPLETE: (plan written; nothing implemented). Four groups were left to brief, not six (see "Why four").

Source: the 1106 table ([../timing-coverage-1106/table.md](../timing-coverage-1106/table.md): TIMING-SWEEP-1105 rows with
time ratio > 1.05 on either class, sweep build next f2157504). Updated table: [table.md](table.md) (generated from the
1106 table and ops.db read-only at 2026-11-06 ~06:30Z; per row: open story or its newest open/landed continuation).

## Coverage now (98 rows)

open 38, re-time 38, new brief 4, deferred 9 (wordexp, OWNER-DEFERRED), out of scope 9 (cold start, COLDSTART-*).
No nscache or libm row is in the table.
* re-time (landed since the 1106 table, not re-timed here): TIMING-IVB-STRSCAN-DISPATCH-2 (strchr/strchrnul/strncmp,
  10 rows), TIMING-IVB-MALLOC-CHAIN-2 (8 malloc/realloc rows), TIMING-COV-REALLOC-GROW-NONAVX2, TIMING-COV-STRCMP-SHORT-AVX2-2
  (strcmp_names, strcasecmp_c_16, strncasecmp_c_64), TIMING-ICONV-TRANSLIT-CPI-4, PARITY-SMALL-ENTRY-1105N-2 (nl_langinfo
  x2), PARITY-SPRINTF-CHK-LS-2, TIMING-N100-COPY-MID / PARITY-COPY-AVX2-3 (memcpy 144/256), plus the 1106 re-time rows.
* open: TIMING-COV-MEMCMP-NONAVX2 and PARITY-MEMCMP-SMALL-EQ-2 (ready), TIMING-IVB-MEMREF-ENTRY-2 (ready), TIMING-N100-MEMCMP
  (ready), TIMING-COV-MEMCHR-SHORT-NONAVX2 (ready), TIMING-N100-SCAN-64K-3 (verifying), TIMING-COV-STRSTR-NONAVX2
  (verifying; TIMING-N100-STRSTR-64 landed), TIMING-COV-GETLINE-LONG-2 (queued_ci), TIMING-N100-MUTEX-MT-2 (queued_ci).
* NONE in the 1106 table and still uncovered: the four rows below. regexec_backref: REGEX-BACKREF-FIXED-2 landed 11-04,
  BEFORE the sweep base (merge 19c027bc is an ancestor of f2157504), so the sweep's 1.18 already includes it;
  REGEX-BACKREF-FIXED-3 was closed unrun.

## Why four, not six

After the 1106 briefs became stories, only four NONE rows remain in scope, one group each. No new group exists below
them in the 1106 table (its cut-off is time > 1.05). Rows between 1.00 and 1.05, or rows from a newer sweep, would need a
new sweep table: none exists after TIMING-SWEEP-1105 (results/ on next).

## Briefs (ranked by gap; none of these calls is in the workload entry table: regexec 0-1 calls, fork/posix_spawn not listed)

| Rank | Brief | Row (time IVB / N100; instr IVB) | Evidence on keke (Zen 1), next c101c4c6 | Stacks on |
|---|---|---|---|---|
| 1 | [TIMING-COV-FORK-COW-FAULTS](TIMING-COV-FORK-COW-FAULTS.md) | fork_exit_wait 1.247 / 1.294; 1.062 | per fork: faults 26.0 vs 12.0, kernel instr 385k vs 297k, same syscalls; after COLDSTART-FORK-CHILD-RESET-2 (its counts): faults 15.1 vs 12.3, kernel 299k vs 283k, user 421 vs 275 | COLDSTART-FORK-CHILD-RESET-2 (finished) |
| 2 | [TIMING-COV-REGEX-BACKREF-SCRATCH](TIMING-COV-REGEX-BACKREF-SCRATCH.md) | regexec_backref 1.181 / 1.012; 1.059 | instr/op 22,938 vs 21,960; L1D misses ~35 vs ~1 per call; run/insert/attempt 61% of instr, memset/fill/copy ~6% | - |
| 3 | [TIMING-COV-REGEX-FIRSTBYTE-IVB](TIMING-COV-REGEX-FIRSTBYTE-IVB.md) | regexec_class_nomatch_1k 1.145 / 0.862; 0.946 | instr/op 8,155 vs 8,669; first_start 52% of instr and cycles; variable shift per byte | - |
| 4 | [TIMING-COV-SPAWN-HEAP256M-IVB](TIMING-COV-SPAWN-HEAP256M-IVB.md) | posix_spawn_true_heap256m 1.493 / 0.999; 1.029 | NOT reproduced on Zen 1: time 0.99, kernel instr/cycles/faults equal; brief reproduces on IVB first, DO NOT LAND if noise | - |

## Method (job keke:tc1106b-profile, role keke:~/w/n1 = next c101c4c6, clean)

Script [tc1106b.sh](tc1106b.sh); raw output [profile-keke-c101c4c6.txt](profile-keke-c101c4c6.txt); compare rows
[compare-rows.txt](compare-rows.txt). `cargo xtask bench --compare --counters --runs 3 cbench_process cbench_patterns`
(staged Liella static vs glibc `.host-dynamic`, both times printed); `perf stat` per row with `CBENCH_ROWS=<row>` minus
`CBENCH_ROWS=none` (instructions:u/k, cycles:u/k, page faults, context switches; L1D and branch misses for regex);
`strace -f -c` of the fork row on both sides (syscall counts, black box); Liella only: `perf record` (instructions:u,
cycles:u; cpu-clock for fork) by symbol. glibc side: aggregate counters, strace counts and timing only.
Caveats: keke is Zen 1, so its times say nothing about Ivy Bridge or the N100 (each brief books its own timing check);
kernel symbols are hidden on keke (kptr_restrict=1), so the fork profile has no kernel breakdown; the cbench_patterns
`none` baseline of the glibc product is large (setup outside the rows), so the per-call L1D/branch-miss figures are
approximate (the brief says to recheck). nico = Ivy Bridge (from the active IVB booking); nozomi = N100 NOT VERIFIED.
