# DL-LIBC-EARLY-5 report

Branch `task/dl-libc-early-5` (= DL-LIBC-EARLY-4's head 02086ac72, then the commits below). Role keke:~/w/n4.
No product code changed in this run.

## Why -4 went red
CI train 599 failed `cargo xtask so`'s STARTUP-DATA-WINDOWS contract (fast: 4 read-only start-up windows > 3; an
unnamed .rodata byte addressed by `stream::State::drain_to` alone in a 64 KiB window). The branch does not touch
`crates/core/src/stream.rs` or any start-up read-only data: its code diff against next is ld.so (`boundary/src/ld.rs`),
the thread/runtime rseq hand-over (`boundary/src/thread.rs`, `runtime.rs`, `rt/src/start.rs`, a few lines), tests and
`xtask/src/so.rs` (`LD_EXPORTS`). The byte was moved by code growth, not added: the start-up layout jitter of owner
ruling #4347, which LAYOUT-DATA-CLUSTERS-4C (landed next 07a4db6ac) removes by clustering the start-up read-only data.

## Commits
* 16c1e78c4 merge of origin/next (ca8aa835a, contains 07a4db6ac) into the branch (clean, no conflicts; brief allows).
* this report, handoff update and report json.

## Jobs (keke, merged head 16c1e78c4; the code is unchanged by the later handoff commits)
* `dl-libc-early-5-lint`: `cargo xtask clippy`, `cargo xtask static`, `cargo xtask records check`, `cargo xtask so`,
  `cargo test -q -p liella --test hot_text_products`: rc 0. `so` printed
  `STARTUP-DATA-WINDOWS ok: 1 read-only start-up data windows of 65536 bytes (ro-budget-fast 1), 12 written start-up
  data pages of 4096 bytes (rw-budget-fast 20); 46 of 46 start-up objects found` and all 12 layout contracts hold;
  hot_text_products 21 passed.
* `dl-libc-early-5-gate`: `cargo xtask affected --run --base origin/next` under `~/bin/gate 16G` (escalates to FULL:
  `xtask/src/so.rs` is in the diff).
* `dl-libc-early-5-plants`: `cargo xtask plants handoff/DL-LIBC-EARLY-4-plants.txt --dir
  /dev/shm/claude/DL-LIBC-EARLY-5/plants-1 --timeout 600` (Zen 1 host RAM dir; WARN-DEV-SHM is only the tool's
  note about tests that mount their own /dev/shm, none of which these plants select).
* `dl-libc-early-5-startup-next`: `cargo xtask bench --counters --only cbench_startup` on a `git archive` of
  origin/next ca8aa835a in /dev/shm.
* `dl-libc-early-5-startup-head`: the same on a `git archive` of the merged head in /dev/shm, then it waits for
  startup-next and fails if any start-up row's instructions ratio vs glibc on the head is above next's.

* `dl-libc-early-5-kinstr`: no build; `bench --counters --only cbench_startup --no-build` on the next and head
  trees, interleaved, 4 times each; prints raw counts and min/max per side and fails if any user-mode start-up row
  (startup_instr_*) of the head is above next's minimum.

## Superseded
* `dl-libc-early-5-startup` and `dl-libc-early-5-startup-cmp`: stopped while still queued (never started, no
  jobs.tsv row): they would have held two slots idle waiting for the gate; replaced by startup-head, which builds
  in its own RAM copy instead of the role.
* `dl-libc-early-5-startup-head` rc 1: its compare failed only on `startup_kinstr_c` (kernel instructions, no
  budget): ratio 0.215 head vs 0.210 next, raw 154870 vs 150821. glibc's own count moved 716593 -> 721491 (+0.7%)
  between the two runs, and the head's spread column read 2%/1%. The strict check was the wrong one for a kernel
  count. The kinstr job measures the run-to-run spread. User-mode rows in that job: equal ratios on all four,
  startup_instr_c 2142 (head) vs 2144 (next).

## Results
* STARTUP-DATA-WINDOWS holds on the merged head with the -4C clustering: 1 read-only window (budget 1). The -4 red
  was layout jitter, not something the change added (see above); nothing to fix, no budget touched.
* hot_text_products on the merged head: 21 passed.
* Start-up instructions vs glibc (keke, same host, next ca8aa835a vs head): instr_c 0.018/0.018 (2144 vs 2142
  user instructions), printf 0.498/0.498, setloc 0.477/0.477, wctype 0.484/0.484; all 12 bench budgets met.
  Kernel instructions, interleaved runs so far: next 152215, 152565; head 152238 (inside next's range).
* Pending at hand-back (long; named in JOBS): gate (FULL by escalation), plants (baselines 1-2 ok when handed
  back), kinstr (all runs).

## Process notes
* A strict "head <= next" check on a kernel-instruction ratio fails on noise; compare user-mode rows strictly and
  kernel rows against a spread measured on interleaved runs.
* Plants on this story take ~10 min per plant (each runs `cargo xtask so` before its test).
