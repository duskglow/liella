# NULLFOLD-COPY-MID: handoff (one page)

State: branch `task/nullfold-copy-mid` from origin/next e8189dd8; code HEAD 714d8265 (+ this handoff commit).
Verdict: INCOMPLETE. The code, tests, D-row and plants file are done. Not done yet: gate and plants results (running
detached), N100 instruction counts, booked timing on the N100 and Ivy Bridge. Timing has the final say.

## Change (D-4.55, owner ruling #3304; amends D-4.22 option B scope for copies)
- `csys/src/machine/fold.S` `.Lmove_129`: `cmpq liella_machine_move_long(%rip), %rdx; ja liella_hot_move_rest` replaces
  `cmpq $191` + both NULL test-and-branch pairs. The word is 256 with folding on, else 0. It goes in the last free 8 bytes
  of the fold-word line (cmp_long's padding), so the line is still one 64-byte line. Kernel bodies unchanged: the first access is
  `(%rsi)` and all loads come before stores (the ordering requirement). 65-128, memset, mempcpy entry unchanged
  (mempcpy above 64 calls the copy entry, so it folds 129-256 too).
- `dispatch.rs`: `FOLD_MOVE` written in `select_null_fold` (dropped first when off, raised after FOLD when on), reset in
  `unresolve_null_fold`; `MOVE_FOLD_MAX = 256`; `resolve_fold_move(n)` (n <= 256). `hot/mem.rs` `move_rest` uses it, so
  the first copy of 1-256 bytes takes the lazy decision. Without it, a process whose copies are all 129-256 would never fold.
- Folding off: 129-256 tail-jump to Rust (`move_rest` -> `move_large`): explicit S6 checks, same EFAULT abort. In that
  rare config these copies take the Rust path and are slower. I chose this over keeping an asm checked path because that path
  would add a compare and a taken branch to every copy above 256 with folding on (local decision, stated in D-4.55).
- `ffi.rs` declaration with its proof obligation.

## Tests (crates/boundary/tests/small_copy.rs)
- New `copies_of_129_to_256_bytes_fault_on_a_null_with_folding_and_abort_without`: every n 129-256, 8 calls (memcpy and
  memmove with NULL d, NULL s, both; __memcpy_chk NULL s; __memmove_chk NULL d), builds (fold off/on x SSE2/AVX2).
- New `a_null_source_of_129_to_256_bytes_writes_nothing_into_a_shared_file_mapping`: MAP_SHARED memfd page; d at the page
  start and at its end; after each crash, the file (pread) and the mapping still hold the pattern.
- Updated: `null_faults_in_the_folded_sizes_and_aborts_in_every_other` (copy calls at 129-256 now SIGSEGV when folded) and
  `the_folding_decision_is_taken_by_the_first_small_call_not_at_startup` (copies above 256 do not decide; memcpy 129 and
  memmove 256 do).
- keke job NULLFOLD-COPY-MID-lint2 rc 0: `cargo xtask clippy`, `cargo xtask static`, all 11 small_copy tests pass.

## Counts (keke, Zen 1; `cargo xtask bench --compare --counters cbench_memref`; instructions Liella / glibc)
before (origin/next e8189dd8, job count-before) -> after (job count-after2):
memcpy_128 28.4/28.4 -> 28.4/28.4; memcpy_144 38.4/36.5 -> 34.4/36.4; memcpy_256 38.4/36.5 -> 34.4/36.4;
memmove_144 38.5/36.5 -> 34.5/36.5; memmove_256 38.5/36.6 -> 34.5/36.5. No row higher. (Time columns are Zen 1, not the arbiter.)

## Jobs (keke) and re-attach
Done: NULLFOLD-COPY-MID-lint2 (rc 0), -count-before (rc 0), -count-after2 (rc 0). Failed setup runs, superseded: -lint
(no cargo on PATH), -gate (no .changed), -gate2 (rc 0, but it only listed its selection: `affected` without `--run` runs
nothing), -plants/-plants2 (refused: unanchored plants), -count-after (wrong bench names).
Running: NULLFOLD-COPY-MID-gate3 (`cargo xtask affected --base origin/next --run` under ~/bin/gate; selected 230 of 236
targets + 117 conformance), NULLFOLD-COPY-MID-plants3 (5 plants). Re-attach: `ssh claude@10.0.100.53 '~/bin/jobwait
NULLFOLD-COPY-MID-gate3 NULLFOLD-COPY-MID-plants3'`; logs ~/logs/NULLFOLD-COPY-MID-{gate3,plants3}.log.

## Next step
1. Collect gate3/plants3 (all 5 plants CAUGHT expected; store-first only by the MAP_SHARED test).
2. N100 counts and booked timing (`~/bin/timing <label> ...` on nozomi for the N100 and the Ivy Bridge host) of
   memcpy_144/256, memmove_144/256 against a staged bench of this branch and of next. Ivy Bridge has no AVX2, so it runs the
   SSE2 129-256 body and loses the same 2 test-and-branch pairs.
Plan rows to read: D-4.22, D-4.52, D-4.55 (plan/07-decisions/s04-byte-arrays-and-strings.md).

## Not to redo
A two-word design (separate AVX2/SSE2 words) does not fit the fold line. The range check cannot come before the word compare
without costing large copies. A one-compare signed/unsigned trick cannot tell folded from checked.

## Slips
The first fold.S edit was a python heredoc, not the Edit tool. The VM role-dir `.synced` head line was updated with `sed -i`.
