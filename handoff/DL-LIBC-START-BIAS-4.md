# DL-LIBC-START-BIAS-4: libc's executable bias without PT_PHDR (start-up counters)

**State:** branch `task/dl-libc-start-bias-4` (continues `task/dl-libc-start-bias-3`, base origin/next 7d31b0621);
role kanon:~/w/kn1. COMPLETE: -3's code is unchanged; this run measured the start-up counters and made a comment
edit in the test (#4537).

**Decisions in force:** D-27.21 (amends D-27.12): `process::exe_bias` uses `liella_rt::start::bias` (D-27.18's
method). D-27.18 and D-27.12 still hold. The code and its record are described in handoff/DL-LIBC-START-BIAS-3.md.

## This run
* `reloc_libc_bias_without_pt_phdr`'s comment now says to name libc.so.6 by path until phase 14's library search lands
  (#4537). No code change.
* clippy and static on the inherited head plus the comment edit: green (`-clippy`, `-static`).
* Direct-tier gate on f67e746c5: 10 suites ok, 9256 passed, 0 failed, conformance 35/35, peak 15G (`-gate`).
* Plants (handoff/DL-LIBC-START-BIAS-3-plants.txt): phdr-only and header-off both CAUGHT, no WARN lines (`-plants`).

## Start-up counters (cbench_startup, Liella instructions:u per process, Broadwell-EP, same host)

| row | head f67e746c5 | origin/next a43eb7c3c | merge-base 7d31b0621 |
|---|---:|---:|---:|
| startup_instr_c | 2150, 2150 | 2150, 2150 | 2150, 2150 |
| startup_instr_printf | 71706, 71706 | 71706, 71706 | 71704, 71706 |
| startup_instr_setloc | 67910, 67913 | 67910, 67907 | 67910, 67906 |
| startup_instr_wctype | 68930, 68934 | 68934, 68930 | 68934, 68937 |

Two runs of each tree (first run: `-startup-head`, `-startup-next`, `-startup-mb`; second run: `-pathlen2`. For
head-1 the second run is a fresh release build of the same source in RAM; for next and the merge-base it re-runs the
same products with `--no-build`). Verdict: **the change moves no counter.** The same products give counts up to 7
instructions apart from one run to the next (merge-base setloc 67910 then 67906, wctype 68934 then 68937), and the
head's counts lie inside next's and the merge-base's ranges. That matches the code: `exe_bias` runs only for
dl_iterate_phdr, _dl_find_object, the `%n` check and the backtrace, never at start-up (`initial_thread` still uses
`start::program`'s single pass). All rows are well under bench/instr_limits.tsv.

## Things not to redo
* `cargo xtask bench --counters --only cbench_startup` is the form (not `--counters cbench_startup`).
* A role's target/liella holds test-profile products after a gate; `bench --no-build` then refuses them. Build
  release products (bench without `--no-build`) in a separate RAM tree.
* Path length of the tree does not explain the few-instruction differences; identical products vary run to run.

## Next step
None for this story. If a later story needs start-up counts to the instruction, it needs several runs per tree
(the counters' `spread` column shows 0% because it rounds; the real spread is about 7 instructions).

Plants file: handoff/DL-LIBC-START-BIAS-3-plants.txt. Read: D-27.18, D-27.12, D-27.21.
