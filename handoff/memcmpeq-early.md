# PERF-MEMCMPEQ-EARLY (-2) (branch task/perf-memcmpeq-early-2)

State: HEAD = the handoff commit on top of dc29baf3 (test fix). Code: csys/src/machine/fold.S `.Lcmp_wide` (memcmp 33-64
AVX2) tests the first block's mask before the last (a4dbfbf1, PERF-MEMCMPEQ-EARLY); register row in docs/saturation/03.

## The red gate of PERF-MEMCMPEQ-EARLY (perf-memcmpeq-early-gate, rc 1) - cause found, not the compare
- s18_4_c_1 failed (plain and interposed): the child reported "open 8" besides "open 6".
- Cause: keke's `~/bin/jobrun` keeps its per-label lock open on fd 8 for the whole job, children included
  (`exec 8>>$LOGS/.locks/$label`, no close-on-exec, by design). Every process the gate starts inherits fd 8, and so
  does a child of posix_spawn (correct: a descriptor without FD_CLOEXEC is inherited). The test checks fds 5-9 and 12
  in the child, so it fails under jobrun on any branch whose affected set includes s18_processes_clauses. A fold.S
  change reaches every test, which is why this branch hit it; CI's launcher on another host does not hold fd 8.
- The memcmp change was re-read: equal -> 0; first block differs -> bsf of (mask0+1); first equal -> bsf of
  (mask1+1) at len-32. No wrong result exists, so there is no failing (length, alignment, position) case to add;
  the three fold.S plants (.Lcmp_last offset, bsr for bsf in last/first) are caught by small_compare/machine_scanners.
- Fix (dc29baf3): the test sets close-on-exec on any of 5-9,12 the test process holds without it
  (`keep_from_children`), so the assertion depends only on the file actions. Plant 4 (close action a no-op in
  crates/core/src/process.rs) must still be caught with "chdir, open, dup2, close in order".

## Jobs on dc29baf3 (keke, role n2; started detached, queue was 1h+ deep at 16:50Z)
- perf-memcmpeq-early-2-gate: clippy + static + direct-tier `cargo xtask affected --base origin/next --run`.
- perf-memcmpeq-early-2-plants: ~/work/perf-memcmpeq-early-2/plants.sh (baseline s18_4_c_1 PASS under jobrun with
  fd 8 open, then plants 1-3 fold.S, 4 process.rs; exits non-zero on a survivor or a failed baseline).
- Earlier: perf-memcmpeq-early-2-check was queued and died with its ssh session before it started (superseded).

## Counts (unchanged from PERF-MEMCMPEQ-EARLY; the code did not change)
memcmp_diff8_64 34.4 -> 30.4 instr (glibc 24.4); other brief rows unchanged; no bench row more instructions.

## Next step (ruling #2517 stands: NULL folding to 2048 approved)
Extend NULL folding (no explicit NULL test; the first load faults on page zero) to the 65-2048 AVX2 classes of the
compare and equality entries (saves ~4 instr on head_1k/head_128/eq rows). Then try a probe-less 33-64 compare (vector
first; check head-difference rows before keeping). Then rework the 65-128 compare class (diff8_96/127).
Do not redo: the first-mask-first reorder; the s18_4_c_1 investigation.

## INFORMATIONAL
- Any test that counts a child's open descriptors is sensitive to a runner's inherited fds; jobrun's fd 8 is one.
  Worth a shared test helper (or jobrun setting FD_CLOEXEC on its lock fd via a tiny helper) - a tooling choice.
