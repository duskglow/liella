# NULLFOLD-COPY-MID: handoff (one page; last run NULLFOLD-COPY-MID-2)

State: branch `task/nullfold-copy-mid-2` (stacks on `task/nullfold-copy-mid`), from origin/next e8189dd8. Code = D-4.55 commit
52c891c5 (the -2 run's alignment commit 4310416c was undone by 5ed9c3e9; fold.S identical to 52c891c5).
Verdict: INCOMPLETE. Correct and tested, faster on Zen 1 (instr) and Alder Lake-N (time), but memcpy_144/256 are ~2.6% slower
in time on Ivy Bridge. Under the brief's rule (no row slower on either class) it must not land as built.

## Change (D-4.55, owner ruling #3304; unchanged from the first run)
fold.S `.Lmove_129`: `cmpq liella_machine_move_long(%rip), %rdx; ja liella_hot_move_rest` replaces `cmpq $191, %rcx` and both
NULL test/branch pairs (word 256 with folding on, else 0; last 8 bytes of the fold-word line). dispatch.rs FOLD_MOVE /
MOVE_FOLD_MAX / resolve_fold_move; hot/mem.rs move_rest decides lazily; tests in crates/boundary/tests/small_copy.rs.

## Results (-2 run)
- keke NULLFOLD-COPY-MID-gate3 rc 0 (code tree 52c891c5: 10 suites ok, 10229 passed, 0 failed; conformance 117/117; peak 12G).
- keke NULLFOLD-COPY-MID-plants3 rc 0: 5 CAUGHT, 0 SURVIVED, 0 WARN-RC-ONLY/NO-MSG (it printed WARN-NOT-RAM: the first run put
  the copy under ~/work, not RAM).
- keke nullfold-copy-mid-2-lint2: workspace clippy -D warnings, xtask clippy, xtask static, small_copy (final head).
- N100 (nozomi, Alder Lake-N), bench counters, base = e8189dd8 code, new = this branch (jobs nullfold-copy-mid-2-n100, -n100-r2;
  5 interleaved passes). Instructions L/G: memcpy_144 38.3 -> 34.3 (glibc 38.2), memcpy_256 38.3 -> 34.3, memmove_144 38.3 ->
  34.3, memmove_256 38.3 -> 34.3, memcpy_128 28.6 = 28.6. Cycles: memcpy_144 14.4-14.5 -> 13.4 (glibc 13.2), memcpy_256
  14.2 -> 13.2 (13.3), memmove_144 27.8 = 27.8, memmove_256 26.8 = 26.8, memcpy_128 9.5 = 9.5. No row slower.
- Ivy Bridge (nico, E3-1220L v2, SSE2 body; jobs nullfold-copy-mid-2-ivb, -ivb-r2; 5 passes). Cycles: memcpy_144 18.8-19.0 ->
  19.4-19.5 (glibc 18.5-18.8), memcpy_256 18.9-19.2 -> 19.4-19.9 (25.5-26.0), memmove_144 35.8 = 35.7, memmove_256 34.7 = 34.8,
  memcpy_128 14.0 = 14.0. Instr 58.5 -> 57.4 only, uops 53.5 -> 49.4. memcpy_144/256 SLOWER.
- Cause, tried: layout ruled out. Base had the SSE2 body at a 64-byte line start by chance; the branch moved it 46 bytes into a
  line. A `.p2align 6` build (stage "pad", SSE2 body back at the base address) measured the same as unaligned on both hosts
  (Ivy Bridge 19.4, N100 13.4), so it was reverted. Model that fits: on Sandy/Ivy Bridge, loads and store addresses share ports
  2/3. The SSE2 body is 16 loads + 16 stores, plus the AVX2 flag load = 33 uops on those ports. The new gate-word load makes 34,
  so +0.5 cycle. memmove rows hide it because another limit binds them. NEGATIVE row in docs/saturation/03.

## Next step (design; needs a fresh timing round on both classes)
Make the SSE2 path read ONE word (as base did) and keep the AVX2 path at <= 2 loads. Options:
(a) one word whose sign bit picks the body: `movq W(%rip),%rcx; testq %rcx,%rcx; js .Lsse2; cmpq %rcx,%rdx; ja rest; avx2` and
    `.Lsse2: btrq $63,%rcx; cmpq %rcx,%rdx; ja rest; sse2`. W = 256 (AVX2, folded), 2^63|256 (SSE2, folded), 0 (off/undecided).
    Both paths: 1 load. AVX2 path +1 instruction vs this branch (Zen 1 35.4, glibc 36.4). The avx2 byte is then not read here.
(b) two words, SSE2 checked first (`cmpq Wsse2,%rdx; jbe sse2; cmpq Wavx2,%rdx; ja rest; avx2`). SSE2: 1 load. AVX2: 2 loads, as
    now. But the fold-word line is full, so Wsse2 needs another line.
Keep the SSE2/AVX2 NULL behaviour the same (no path-dependent result). Update plant fold-unguarded-asm (its pattern names the
cmpq). Time memcpy_144/256 (memcpy_128 as control) on N100 and Ivy Bridge, base/new interleaved: scripts keke:~/work/
NULLFOLD-COPY-MID-2/{stage.sh,stage_base.sh} (stage_base reverses branch.patch in the role, then re-applies it with fresh mtimes),
timing hosts ~/work/NULLFOLD-COPY-MID-2/run2.sh (stages base/new/pad there; edit the stage list).
Plan rows: D-4.22, D-4.52, D-4.55 (plan/07-decisions/s04-byte-arrays-and-strings.md).

## Not to redo
Two-word with the AVX2 word first (SSE2 still 2 loads). Code alignment of the SSE2 body (measured, no effect). Keeping the
explicit NULL tests on SSE2 only (path-dependent NULL result). The first run's notes: the range check cannot come before the word
compare without costing large copies; a one-compare signed/unsigned trick cannot tell folded from checked.

## Jobs of the -2 run
Final: keke NULLFOLD-COPY-MID-gate3, keke NULLFOLD-COPY-MID-plants3, keke nullfold-copy-mid-2-lint2, nozomi nullfold-copy-mid-2-n100,
-n100-r2, nico nullfold-copy-mid-2-ivb, -ivb-r2; stage builds keke nullfold-copy-mid-2-stage-new/-base/-pad (rc 0).
Superseded: keke nullfold-copy-mid-2-lint (rc 0, head 7192f508 before the alignment experiment), nullfold-copy-mid-2-gate and
-plants (rc 143: stopped by me, they were testing the alignment commit, which was then reverted). Bookings 64/65 cancelled
(the `timing` queue ran the jobs alone).

## Slips (-2 run)
One empty no-op `python3 - <<'X'` heredoc call (did nothing). The first sync passed an IP for the host and failed with a tool error
(it takes host names); not a permission refusal.
