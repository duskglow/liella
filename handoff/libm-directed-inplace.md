# LIBM-DIRECTED-INPLACE(-2): the double exp quick phase in the caller's directed rounding

State: branch task/libm-directed-inplace-2 (cut from origin/next bb9130ed9; carries LIBM-DIRECTED-INPLACE's WIP
commits), role keke:~/w/r5. Decision D-10.94 written (records/decisions/D-10/D-10.94.toml). Verdict of run -2:
INCOMPLETE: LAYOUT-JITTER (the gate's only test reds are the start-up localtime_r page-fault bounds; see below).

## Done
- Run 1 (LIBM-DIRECTED-INPLACE): `fast64::quick::exp_directed` (both builds), its proof (module doc of
  crates/math/src/fast64/quick.rs, "Directed rounding"; `directed_bounds` test), `hot::exp_directed`,
  `mathret::elem_fast_dir` / cold `elem_directed`, the `exp d` row on elem_fast_dir.
- Run 2: `selftest::exp_directed` (out of line, for tests that set MXCSR around it); tests in
  crates/boundary/tests/exp_quick.rs: `exp_directed_declines_at_breakpoints_and_the_entry_rounds_them` (x = ±2^-k,
  e^x within x²/2 of a directed boundary: the quick phase must decline, the entry must round right, every directed
  mode, both builds), `exp_directed_rounds_correctly_in_both_builds` (30000 operands x 3 modes x 2 builds, quick and
  entry against `Real::exp(x, mode)`, decline limits 3% SSE2 / 0.2% FMA), ignored `exp_directed_accuracy_job`
  (EXP_DIR_N); s10_math_symbols comment updated (exp now in place under a rounding-only word); D-10.94 record
  (Tang 1989; Boldo, Graillat & Muller 2017). expf unchanged (already one-pass in the caller's direction, D-10.63/72).

## Results (keke, Zen 1)
- lint0 (inherited head 863263360): clippy + static rc 0. tests1: exp_quick 10 passed, s10_math_symbols 177,
  directed_bounds 1 (rc 0).
- plants1: 4 of 4 CAUGHT with msg (handoff/LIBM-DIRECTED-INPLACE-2-plants.txt), no WARN lines.
- acc1: 10^7 operands per distribution ([-10,10], uniform bits), direction (nearest, down, up, zero) and build:
  0 mismatches. Quick fallback SSE2 0.80-0.84%, FMA 0.010-0.011%.
- gate1 (affected --base origin/next, head 1b1df9d59): 7941 passed, 4 failed (2 tests x 2 runs):
  hot_text_products `init_first_localtime_r_stays_within_its_page_fault_bound` (21 > 20) and
  `init_first_localtime_r_and_mktime_stay_within_their_combined_page_fault_bound` (21 > 20); and `xtask static`
  (bare `|` in D-10.94, fixed in 75cd24c22). This change touches no start-up code: brief rule "START-UP LAYOUT
  JITTER" (#4347). Same count on the base revision bb9130ed9: see the hand-back / report json (basefaults job).
- Counts (cbench_libm --compare --counters --runs 1, instructions per call Liella / glibc; time ns Liella / glibc
  on an UNBOOKED build host, not a verdict):
  | row | FMA instr | FMA time | SSE2 instr | SSE2 time |
  | exp_upward | 107.5 / 67.1 | 53.4 / 10.5 | 125.1 / 77.1 | 55.9 / 12.3 |
  | exp (nearest) | 85.2 / 67.1 | 19.0 / 10.4 | 99.4 / 77.1 | 21.5 / 12.5 |
  | expf_upward | 55.1 / 39.1 | 8.65 / 5.83 | 55.1 / 43.1 | 8.64 / 6.10 |
  Before (docs/benchmarks.md, older revision): exp_upward 408 instructions. exp_upward is now nearest + 22-26
  instructions, but ~34 ns slower than nearest on this host: the time is NOT explained by the instructions.
- NOT DONE: booked timing (Ivy Bridge nico/kotori, N100 nozomi), hot_text_products on the merged head beyond the
  gate, exhaustive expf (expf code unchanged by this story).

## Open questions
- The ~34 ns gap of exp_upward over exp on Zen 1 with only ~24 more instructions: profile (perf stat cycles,
  machine clears / microcode-assisted ops; perf record on the cbench row) before any further design.

## Next step
1. When the layout fix (LAYOUT-TEXT-PIN / LAYOUT-DATA-CLUSTERS) lands: re-gate `affected --base origin/next`.
2. Profile the exp_upward time gap (above), then booked timing on Ivy Bridge and N100 (target exp/expf upward <= 2x).
3. LIBM-DIRECTED-INPLACE-3: log/logf, sin/sinf, pow with the same recipe (check the directed rounding of each
   reduction's integer-producing SHIFT step).
