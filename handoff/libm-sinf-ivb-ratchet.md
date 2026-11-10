# LIBM-SINF-IVB-RATCHET(-2): handoff (sinf's Ivy Bridge loss from the bit-built trig operand, owner #4424)

State: branch `task/libm-sinf-ivb-ratchet-2` (stacks on `task/libm-sinf-ivb-ratchet`), WIP, NOT READY. Role kanon:~/w/kn1.
Decisions: D-10.67, D-10.68, D-10.72 in force; D-10.79 written (records/decisions/D-10/D-10.79.toml, amends D-10.67);
pending saturation record records/pending/LIBM-SINF-IVB-RATCHET-2-1.toml (SR-NEW, section 1: the NEGATIVE Estrin trial;
the coordinator assigns the SR id). Report: handoff/reports/LIBM-SINF-IVB-RATCHET-2.json.

## Done in -2 (coordinator ruling #4434, option (a))
* `hot/fpregs.rs`: `widen(x: f32) -> f64`, one `cvtss2sd xmm0, xmm0` (`in("xmm0") x, lateout("xmm0") y`, `nomem`,
  not `pure` so it stays behind the range test). `inout(xmm_reg) x => y` is refused by rustc (f32/f64 differ in size),
  hence the named register (the psABI passes the `float` argument in %xmm0).
* `hot/f32_trig.rs`: `sinf/cosf/tanf_nearest` pass `widen` to new `fast32::{sin,cos,tan}_nearest_widen` ->
  `trig_core_with(x, widen)`; `trig_core` = `trig_core_with(x, widen_bits)` (sincosf, cexpf, Bessel unchanged).
  `s10_math.tsv` rows sin/cos/tan `f` point at `crate::hot::*f_nearest`. `cargo xtask math-gen` must be rerun and its
  output (`s10_math_gen.rs`, maybe csys adapters) COMMITTED: the job regenerated it on the role only; not in the branch yet.
* Tests: `fast32::tests::trig_widen_entries_match_the_bit_built_ones` (sampled, widen entries == bit-built entries);
  `partial_write::tests::in_place_conversion_passes_and_another_destination_does_not` (dst == src passes with nothing
  owned; dst != src reported: the rule already had the exemption, now tested and documented);
  `policy::machine_register_tests` (3 tests: the widening passes, other dst refused, missing `nomem` refused).
* `xtask/src/policy.rs`: machine-register allows `WIDEN_TEMPLATE = "cvtss2sd xmm0, xmm0"` with `WIDEN_OPERANDS`;
  explicit-register literals (right after `(`) are no longer read as templates.

## Job results (kanon)
* lint0 (inherited head): clippy + static green.
* build3 (head before this note): math-gen, records check, `cargo xtask so`, clippy OK; partial-write 0 findings (791
  exports); `cargo xtask static` RED: unsafe-audit 12 violations, incl. `fpregs.rs:117: machine-register template not
  a plain string literal` and `fpregs.rs: 0 MXCSR stores` -> the audit's own parser (not machine_register_templates'
  new code alone; the other 10 lines are above line 102 of ~/logs/libm-sinf-ivb-ratchet-2-build3.log, unread) trips
  on the new block, probably the `"xmm0"` literals or the two-line comment between `#[allow]`/`unsafe`. The unit
  tests after static did not run (set -e).

## Next step (exact)
1. `grep -n "fpregs\|unsafe-audit" ~/logs/libm-sinf-ivb-ratchet-2-build3.log` on kanon; fix the audit findings (likely
   the comment placement in `widen` and the audit's string-literal scan of `in("xmm0")`); fix D-10.79's body (it still
   says `cvtss2sd {x}, {x}` / `inout(xmm_reg)`: now `cvtss2sd xmm0, xmm0`, `in/lateout("xmm0")`).
2. Rerun ~/work/LIBM-SINF-IVB-RATCHET-2/build.sh as a job; commit the regenerated s10_math_gen.rs; check sinf's
   disassembly has `cvtss2sd %xmm0,%xmm0` and no extra move on the hot path.
3. Plants (`cargo xtask plants`): partial_write true_input disabled; dst != src exempted; policy WIDEN check dropped;
   `sin_nearest_widen` using `.1`. Counts, then booked interleaved timing on Ivy Bridge + N100 (sinf, cosf, tanf,
   asinhf, acoshf): gate sinf IVB <= 1.78x, other gains kept. Then affected --base origin/next.

## Do not redo
* The DSB/MITE and RS counters, the stack-offset sweep, the Estrin trial (NEGATIVE, recorded pending).
