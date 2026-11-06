# TIMING-COV-FORK-COW-FAULTS: fork + _exit + waitpid costs more page faults and kernel work than glibc

Id: TIMING-COV-FORK-COW-FAULTS
Kind: code
Priority: 60
Size: M
Host: -
Stacks-on: COLDSTART-FORK-CHILD-RESET-2
Model: opus
Effort: high
BUDGET: 80 tool calls / 160k

## The change

`cbench_process/fork_exit_wait` (fork, child `_exit(0)`, parent `waitpid`) is slower than glibc in time on both
classes in the 1105 sweep: Ivy Bridge 1.247, N100 1.294 (instr:u 1.062 / 1.081). Once COLDSTART-FORK-CHILD-RESET-2
(finished, not landed) is in, the user-space gap is small (421 vs 275 instr per fork). What is left is in the kernel,
and it comes from the process image: 15.1 vs 12.3 page faults per fork and 299k vs 283k kernel instructions per fork
(that story's counts). Cut the faults and the kernel work that Liella's memory layout and fork/exit write pattern cause,
without changing behaviour. Work on top of COLDSTART-FORK-CHILD-RESET-2's branch (merge origin/next once it lands).

## Rows

| row | instr IVB | time IVB | instr N100 | time N100 |
|---|---|---|---|---|
| cbench_process/fork_exit_wait | 1.062 | 1.247 | 1.081 | 1.294 |

## Evidence (TIMING-COVERAGE-1106B, keke = Zen 1, next c101c4c6, job keke:tc1106b-profile; raw: profile-keke-c101c4c6.txt)

`perf stat` of the staged products (`cbench_process` Liella static vs `.host-dynamic` glibc), `CBENCH_ROWS=fork_exit_wait`
minus `CBENCH_ROWS=none`, per fork (7000 forks; parent + child, children inherited). This is next WITHOUT the reset story:

| per fork | Liella | glibc |
|---|---|---|
| instructions:u | 6,801 | 290 |
| instructions:k | 384,971 | 297,250 |
| cycles:k | 593,804 | 455,964 |
| page faults | 26.0 | 12.0 |
| syscalls (strace -f -c) | clone 1, wait4 1, set_robust_list 1 (child) | the same |

`cargo xtask bench --compare --counters --runs 3 cbench_process` on keke: glibc 236,054 ns, Liella 278,582 ns (ratio 1.18;
keke time is Zen 1 only, it says nothing about IVB/N100), instr/op 200,384 vs 190,362 (1.05: the bench's counter includes
the parent's share of kernel work in this row). Same syscalls on both sides, so the gap is not an extra call: it is page
faults (copy-on-write and first touch) and the cost of `clone` copying the address space. At start the Liella static image has
11 mappings, glibc dynamic 23 (`/proc/<pid>/maps`), so the number of mappings is not the cause; the number of
present, writable private pages (which `clone` must write-protect and copy page tables for, and which later fault on
write) is the candidate. Liella samples (cpu-clock, user+kernel): kernel symbols are hidden on keke (kptr_restrict=1),
`child_reset` 2.8% (the reset story removes it). vfork_exit_wait is at parity (0.94 time, 0.99 instr), so wait4 and exit
are not the issue.

Decision rows checked: D-18.7 (daemon uses Liella's fork), D-22.12 (atfork / child reset), D-26.2, D-25.5. None fixes the
image layout or the fork write pattern.

## Hypothesis (to test first, before any code)

1. Liella writes more distinct pages around fork than glibc: in the parent before `clone` (locks, caches, pid word),
   in the child (reset, exit path: atexit list, stdio flush walk, TLS/thread block), and in the parent after `clone`
   returns. Each written page that `clone` made copy-on-write costs one fault. Measure: faults in the parent alone vs
   in the child alone (`perf stat` without inherit, `-i`, plus `perf stat -e page-faults` on the child via a probe
   that calls `fork` in a loop without `_exit`), and `/proc/self/smaps_rollup` (Anonymous, Private_Dirty) of the
   two products at the row start.
2. Then gather the words that fork and the child's exit write onto as few pages as possible (one "fork-hot" page),
   and make the child skip writes whose value does not change (the reset story started this). Also check that the
   static image does not dirty pages at start-up that it never needs (relocation of read-only data, eager zeroing
   of .bss-like arenas, a malloc arena touched before first use): fewer dirty private pages = fewer PTEs for `clone`
   to copy and write-protect.

## Tests / plants

Behaviour tests of fork stay green (fork child state, atfork order, stdio after fork, robust list). Add a test that
the child's view after fork is unchanged for every word moved (spec §18 fork clauses; name them). Plants through
`cargo xtask plants` (docs/plants.md), msg:<regex> on each: a moved word not reset in the child, a "skip unchanged"
check inverted, a fork-hot page not reinitialised.

## JOBS

gate (direct tier, `cargo xtask affected --run`, base origin/next), plants, counts before/after
(`perf stat -e instructions:u,instructions:k,page-faults` per fork as above, both products, and
`cargo xtask bench --compare --counters --runs 3 cbench_process`), and a booked timing check of the row on both classes:
`nico:<story>-timing-ivb` and `nozomi:<story>-timing-n100` (host class of nozomi = N100 is NOT VERIFIED: check with ops),
each `~/bin/timing <label> cargo xtask bench --compare --no-build --runs 5 cbench_process`.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
