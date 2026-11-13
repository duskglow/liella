# DL-LIBC-EARLY-6 report

Branch `task/dl-libc-early-6` = `task/dl-libc-early-5` head d4d21d28f (merge of next ca8aa835a at 16c1e78c4; code
unchanged since c914ae2c9) plus this report and the handoff update. Role shiki:~/w/sh2 (Haswell, 26 vCPU).
No product code changed in this run: no regression showed.

## Why -5 went red
Its `dl-libc-early-5-kinstr` job compared the head's MAXIMUM against next's MINIMUM on the user-mode start-up rows.
Those counts move by about 10 instructions from run to run on both sides, so overlapping ranges always read WORSE
(setloc head 68056-68067 vs next 68058-68066). No code fault. Note: -5's `dl-libc-early-5-gate` (FULL) and
`dl-libc-early-5-plants` did run, both rc 0 on keke (finished 18:59 and 19:09 on 11-11, after the red verdict).
This run repeats both on its own head, as the brief asks.

## The fixed compare (job dl-libc-early-6-kinstr, shiki, script ~/work/DL-LIBC-EARLY-6/kinstr.sh)
`git archive` of next ca8aa835a (the next the branch merged) and of the head, built once each in
/mnt/zram, then `bench --counters --only cbench_startup --no-build` 6 times per side, interleaved. A user-mode
row (startup_instr_*) is WORSE only when the head MEDIAN is above next's MAXIMUM. startup_kinstr_c (kernel
instructions, no budget) is reported as min/median/max and is never failed. rc 0.

| row | next min / median / max | head min / median / max | verdict |
|---|---|---|---|
| startup_instr_c | 2146 / 2146 / 2146 | 2146 / 2146 / 2146 | ok |
| startup_instr_printf | 71685 / 71692.5 / 71695 | 71680 / 71692.5 / 71706 | ok |
| startup_instr_setloc | 67887 / 67894 / 67895 | 67887 / 67888 / 67895 | ok |
| startup_instr_wctype | 68911 / 68911.5 / 68918 | 68911 / 68915.5 / 68918 | ok |
| startup_kinstr_c | 146168 / 146396 / 147151 | 146325 / 146592 / 149996 | reported |

Kernel-mode spread: the medians differ by +196 instructions (+0.13%). On each side one run sits apart from the
rest (next 147151, head 149996, head run 5). glibc's own kernel count moved 684999-693300 (1.2%) over the same
runs. The +2% seen on keke in -5 (head 152238-155056 vs next 149326-152565, 4 runs each) did not show again
here: it is noise across reruns (single high runs), not new kernel work. That agrees with the code: the static
start path is unchanged (`register_kernel_areas` gets the same `RSEQ_OWNED` value). The absolute counts differ
from keke's because the CPU class differs (Haswell here, Zen 1 there).

## Jobs (shiki)
* `dl-libc-early-6-lint`: `cargo xtask clippy`, `cargo xtask static`, `cargo xtask records check` on the
  inherited head: rc 0.
* `dl-libc-early-6-kinstr`: above, rc 0.
* `dl-libc-early-6-gate`: `cargo xtask affected --run --base origin/next` under `~/bin/gate 16G` (FULL by
  escalation: `xtask/src/so.rs` is in the diff). Pending at hand-back.
* `dl-libc-early-6-plants`: `cargo xtask plants handoff/DL-LIBC-EARLY-4-plants.txt --dir
  /mnt/zram/claude/DL-LIBC-EARLY-6/plants-1 --timeout 600`. Pending at hand-back.

## Process notes
* Run-to-run noise in start-up counts: compare a median against the other side's range, never max vs min.
  One run in six can sit 2-3% high in kernel instructions on either side.
