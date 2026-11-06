# NULLFOLD-COPY-MID: handoff (one page; last run NULLFOLD-COPY-MID-3)

State: branch `task/nullfold-copy-mid-3` (stacks on `task/nullfold-copy-mid-2`, `task/nullfold-copy-mid`), from origin/next
e8189dd8. Code commits of this run: 4fadf32d (one-word gate), 0e639269 (test child order); docs dff8ad43; handoff after.
Verdict: COMPLETE. The D-4.55 129-256 copy folding with a one-word gate: no timing row slower on Ivy Bridge or Alder Lake-N.

## Change (this run, on top of D-4.55)
fold.S `.Lmove_129`: `movq liella_machine_move_long(%rip),%rcx; testq; js .Lmove_129_sse2_gate; cmpq %rcx,%rdx; ja rest;`
AVX2 body; `.Lmove_129_sse2_gate: btrq $63,%rcx; cmpq; ja rest;` SSE2 body. Word = 256 (folding + AVX2), 2^63|256 (folding +
SSE2), 0 (off). One load on both paths (the AVX2 flag byte is no longer read there; `liella_machine_move_mid` still tests it).
dispatch.rs: `MOVE_SSE2`, `move_word`, `store_move_word` (stored before folding or the AVX2 flag drops, after both are set;
`select` and `select_null_fold` both write it). selftest `copy_class_word`. Test
`the_129_to_256_copy_word_follows_the_folding_and_build_settings_in_either_order` (both bodies copy the same bytes, so the
word is the only witness on AVX2 hardware). The 129-256 NULL test children now set the build first, then folding (production
order); the old order hid the fold-unguarded-word plant once `select` rewrote the word.

## Results (this run)
- Timing, base (e8189dd8 code) / new (one word) / old (the -2 run's two-word gate), 3 interleaved passes, bench counters.
  Ivy Bridge (nico, job nullfold-copy-mid-3-nico): memcpy_144 18.8-18.9 vs base 18.8-19.0 (old 19.5; glibc 18.5-18.9);
  memcpy_256 18.9 vs 18.9 (old 19.4-19.5; glibc 25.6-25.9); memmove_144 35.7-35.8 vs 35.8-36.0; memmove_256 34.7-34.8 vs
  34.7-34.9; memcpy_128 14.0 vs 14.0-14.2. Liella instr 51.4 vs 53.5 (old 49.4).
  Alder Lake-N (nozomi, job nullfold-copy-mid-3-nozomi): memcpy_144 12.2-12.4 vs 14.4 (old 13.4; glibc 13.2-13.4); memcpy_256
  12.2-12.3 vs 14.2 (old 13.2; glibc 13.2-13.3); memmove_144 27.7-27.9 vs 27.8; memmove_256 26.8-27.0 vs 26.8 (27.0 once, 26.8
  in passes 2-3); memcpy_128 9.5 = 9.5. Instr 35.3 vs 38.3 (old 34.3). No row slower. The stage of the new build is commit
  4fadf32d; later commits change tests and docs only.
- keke nullfold-copy-mid-3-lint (head 4fadf32d): workspace clippy, xtask clippy, xtask static, small_copy, bench stage: rc 0.
- keke nullfold-copy-mid-3-gate2 (head 0e639269, direct tier): see the report json / hand-back for the final line.
- keke nullfold-copy-mid-3-plants3 (fixed plants file; code tree 0e639269): see the report json. plants2 (same plants, regex
  `\d` in two msg fields) was 7 CAUGHT, 0 SURVIVED, 2 WARN-NO-MSG: both failed on the intended assertion (`fold true avx2
  false call 0 n 257`, SIGSEGV for the abort); grep -E has no `\d`, now `[0-9]+`.
- keke nullfold-copy-mid-3-final: workspace clippy + xtask static on the final synced head.

## Next step
None required: land (with its two parent branches). Optional: Zen 1 instruction row for memcpy_144/256 (expected 35.4,
glibc 36.4; the AVX2 path has one instruction more than the two-word gate, one load fewer).
Plan rows: D-4.22, D-4.52, D-4.55 (plan/07-decisions/s04-byte-arrays-and-strings.md); saturation docs/saturation/03.

## Not to redo
Two-word gates (either order); code alignment of the SSE2 body (no effect); explicit NULL tests on one build only
(path-dependent NULL result); a one-compare signed/unsigned trick (cannot tell folded from checked; any second outcome
from one compare moves the 256 bound).

## Jobs of this run
Final: keke nullfold-copy-mid-3-gate2, keke nullfold-copy-mid-3-plants3, keke nullfold-copy-mid-3-final, nico
nullfold-copy-mid-3-nico, nozomi nullfold-copy-mid-3-nozomi (timing via ~/bin/timing, no booking made), keke
nullfold-copy-mid-3-lint (stage build).
Superseded: keke nullfold-copy-mid-3-gate (rc 143, stopped by me: head 4fadf32d before the test-order fix),
nullfold-copy-mid-3-plants (stopped: fold-unguarded-word SURVIVED there, fixed by 0e639269), nullfold-copy-mid-3-plants2
(rc 0, two WARN-NO-MSG from the `\d` regex).

## Slips (this run)
One empty no-op heredoc call (did nothing). One sync refused by the tool while the stale plants job held the role (then
stopped with kill-label and re-synced); not a permission refusal.
