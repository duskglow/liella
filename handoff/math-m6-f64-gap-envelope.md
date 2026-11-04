# MATH-M6-F64-GAP-ENVELOPE (register 16.5 #3 + #4), run 2 (ENVELOPE-2): fused, counted, gate/plants started

State: branch task/math-m6-f64-gap-envelope-2 (stacks on task/math-m6-f64-gap-envelope), code head f901a9e4, cut from
origin/next 432f1ef7. Role shiki:~/w/sh2. Scratch on shiki: ~/work/math-m6-f64-gap-envelope-2/ (counts-*.txt, plants.sh,
counts.sh, logs); run 1's disassembly under ~/work/math-m6-f64-gap-envelope/{base,after,after2}/ (dis.sh).

## Done
- Run 1: `#[inline(always)]` kernels and entries (fast64.rs, #3); `elem_fast_cold` out of line (mathret.rs, #4).
- Run 2: s10_math.tsv passes `|x| math::fast64::<fn>_nearest(x)` for exp, exp2, log, log2, log10 (a fn item went through
  the non-inlined `call_once` shim); s10_math_gen.rs regenerated. math-gen now emits `clippy::redundant_closure` in the
  allow (xtask/src/math.rs, with the reason). Disassembly (after2/): every one of the six entries is one fused function;
  calls left: `liella_machine_mxcsr_get` (hot), `elem_fast_cold` (fallback), `fast64::settle` (near-miss only).
- clippy (--workspace --all-targets --all-features -D warnings) rc 0 and `cargo xtask static` rc 0 on f901a9e4.

## Counts (instructions:u per call, base 432f1ef7 -> f901a9e4, glibc; job math-m6-f64-gap-envelope-2-counts, rc 0)
Default: exp 194.3->172.3 (67.1), exp2 214.3->186.3 (51.1), log 236.5->207.4 (63.1), log2 241.5->228.5 (63.1),
log10 255.6->242.7 (102.9), pow 521.5->481.4 (141.2), exp_upward 417.7->404.9, log_upward 407.3->386.4,
pow_upward 936.6->913.4. -AVX2: the same within 0.1 (glibc 77.1/51.1/78.1/83.1/118.3/191.3).
SLOWER (both levels): exp_qnan 98.2->111.1, exp_overflow_errno 209.2->223.2, exp_tiny 444.9->457.4 (the inputs that
leave the fast domain now pay the out-of-line cold call), and expf_qnan 105.2->114.1 (f32: not explained yet; elem32_fast
was not changed). Other rows: big-count rows move by under 0.1% both ways (noise).
MXCSR read (ruling #2350): the call stays; it is about 4 of 172 instructions on exp (call/ret plus a spill), so not a
large share; no asm category proposed.

## Open
- RULING NEEDED: keep the trade-off (common rows -13..-40, special-input rows +9..+14)? Recommendation: keep (favour the
  common case) and try a qnan/inf pre-check ahead of the cold call next run so those rows also stop being slower.
- expf_qnan +9: find out why an f32 row moved (shared code in mathret? layout?) before landing.
- Gate: `cargo xtask affected --run` ESCALATED to FULL on its own (tsv generator input, generated file, xtask changed);
  job math-m6-f64-gap-envelope-2-gate, started detached, not waited for.
- Plants: math-m6-f64-gap-envelope-2-plants-2 (fresh RAM copy plants-2): skip-mode-check and wrong-rounding-dispatch
  expected CAUGHT, drop-black-box expected EQUIVALENT (ordering only visible under an unmasked trap). The first plants job
  had a baseline rc 2: s10_math_symbols needs `cargo xtask so` first (fixed in plants.sh).
- NOT DONE: the sweep digest (step 3 of run 1's list) and the detached full sweep; register 13.5/16.5 rows and the D-10.13
  note with the measured saving; the remaining `push rbx` (step 2).

## Exact next step
1. Read the gate and plants-2 rows in shiki ~/logs/jobs.tsv; fix any red in the direct math tests.
2. Explain expf_qnan; try the pre-check for the special-input rows; recount (counts.sh, base copy is in
   /dev/shm/claude/MATH-M6-F64-GAP-ENVELOPE-2/base, already built).
3. Digest in double_fast_entry_mismatches, base vs after with LIELLA_FE_SWEEP_CHUNKS=10, then the full sweep detached.
4. Register rows and D-10.13 note.

## Not to redo
- Base build and base counts (432f1ef7) exist on shiki; the fused form is confirmed by disassembly.
- The Edit tool trims a trailing space in a replace_all new_string; there is no local cargo (run cargo on the role).
