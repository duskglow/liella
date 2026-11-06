# TIMING-COV-FORK-COW-FAULTS: handoff

Branch `task/timing-cov-fork-cow-faults`, stacked on `task/coldstart-fork-child-reset-2` (187d1daa; not landed yet,
no merge done). Role keke:~/w/n3. Code commit 5186b8a4 "Group the fork child's text path in the hot text".

## Where the faults were (profile before any code; keke = Zen 1 host)
`perf record -e page-faults -c 1 -d` over 1000 fork+_exit+waitpid (probe forkf.c, no ASLR), each fault placed in its
mapping (scripts keke:~/work/TIMING-COV-FORK-COW-FAULTS/{forkf.c,faults.py,profile.sh}):
- parent: 2 faults per fork (stack page, a data page) for Liella, glibc dynamic and glibc static alike.
- child, Liella fast products: 13 = 9 text + 4 writes (stack, thread block TID by CLONE_CHILD_SETTID, .data, heap
  cache in park_others). glibc static: 7 = 3 text + 4 writes. The copy-on-write writes were already at parity.
- Cause: `fork` copies no page tables of file-backed text, so the child refaults each 64 KiB fault-around window its
  path touches. The fork path (`__fork`, `fork::fork`, child_reset hooks, `_Exit`, `exit_now`) was not in the hot list
  because the gdb tracer followed the parent at fork.
- smaps_rollup at start: Liella Anonymous 44 kB, glibc static 52 kB, glibc dynamic 88 kB: no excess dirty anonymous
  memory for `clone` to copy (the 620 kB Private_Dirty is the tmpfs-backed binary, not anonymous memory).
- The brief's 15.1 faults were measured on fast (test) products; the bench's release products read 10.0 before.

## Change (D-NEW in plan/07-decisions/x-implementation-decisions.md; placeholder, no id reserved in the brief)
`trace_gdb.py` follows the fork child when `HOT_TEXT_FOLLOW_FORK=child`; `regen.sh` traces the new probe
`tools/hottext/probes/p_fork.c` that way; `hot-text.list` regenerated (fast, then release trace; +16 keys: 11 fork-path
keys in one block, plus `__waitpid`, `spawn::wait4`, and 5 refreshed names of already-hot functions whose module path
moved). Oracle `page_faults.c` gains a "fork" mode (second child's ru_minflt via wait4); new test
`init_fork_child_stays_within_its_page_fault_bound` (no spec clause: product layout), FORK_BOUND 9.
No words moved and no behaviour changed, so the brief's "child view unchanged for every moved word" test and its three
word-reset plants have no subject; the plants guard the layout instead.

## Results
- Counts (keke:timing-cov-fork-cow-faults-regen rc 0; per extra fork+_exit+waitpid, (10 - 1 forks)/9, perf stat -r 20):
  | | user instr | kernel instr | page faults |
  |---|---|---|---|
  | Liella release, before (reset-2 head) | 400 | 231,661 | 10.0 |
  | Liella release, after | 397 | 202,878 | 7.2 |
  | glibc 2.39 dynamic | 276 | 275,691 | 12.0 |
  | glibc 2.39 static | 265 | 212,602 | 8.9 |
  Child faults (new test): release 8 -> 6, fast 13 -> 7.
- keke:timing-cov-fork-cow-faults-check rc 0: cargo xtask clippy, cargo xtask static, workspace clippy -D warnings.
- keke:timing-cov-fork-cow-faults-plants rc 0: 2 CAUGHT, 0 SURVIVED, 0 WARN (plants file
  keke:~/work/TIMING-COV-FORK-COW-FAULTS/plants.txt: fork-path keys dropped, reset-hook keys dropped).
- keke:timing-cov-fork-cow-faults-gate: RUNNING at hand-back. `cargo xtask affected --run --base origin/next` escalated itself to FULL ("crates/libc-so/hot-text.list: a product crate or its linker input"): workspace clippy + `cargo xtask test`. Started 08:44Z by keke:~/work/TIMING-COV-FORK-COW-FAULTS/jobs.sh (sequential: check, gate, bench, stage).
- keke:timing-cov-fork-cow-faults-bench (`cargo xtask bench --compare --counters --runs 3 cbench_process`) and keke:timing-cov-fork-cow-faults-stage (stage-after.tar for timing): queued behind the gate in jobs.sh; not read.
- Timing: NOT STARTED. Ready on kotori (Ivy Bridge; nico was busy with cfcr2-ivb, 2h53m) and nozomi (Intel N100, checked): ~/work/TIMING-COV-FORK-COW-FAULTS/{timing.sh,stage-before.tar} (before = reset-2 head stage). Next: after the stage job, copy keke:~/work/TIMING-COV-FORK-COW-FAULTS/stage-after.tar to both as stage-after.tar, then `ssh <host> 'setsid nohup ~/bin/timing timing-cov-fork-cow-faults-ivb bash ~/work/TIMING-COV-FORK-COW-FAULTS/timing.sh </dev/null >/dev/null 2>&1 &'` (label -n100 on nozomi).

## Open / next
- The remaining user-instruction gap (397 vs 276) is in the reset story's area (child_reset, park_others).
- Not to redo: the fault profile above, the list regen.
- Plan rows: D-X.20, D-NEW (this), D-18.8.
