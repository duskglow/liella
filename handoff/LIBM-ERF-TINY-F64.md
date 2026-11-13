# LIBM-ERF-TINY-F64: f64 erf for |x| < 2^-55 in place (handoff + report)

State: branch task/libm-erf-tiny-f64-2 (continues task/libm-erf-tiny-f64), code head 0ccb708ac; role shiki:~/w/sh2
(Haswell-EP, FMA). Decision: records/pending/LIBM-ERF-TINY-F64-1.toml (D-NEW, series D-10; ruling #5029 reserves
D-10.113, the coordinator assigns it). Run 2 (LIBM-ERF-TINY-F64-2) finished the story.

## Done (run 1)
* `fast_special::erf_nearest` (both builds): `|x| < 2^-55` (and ±0) answered inside the existing
  `!(LINEAR..).contains(&ax)` refusal branch (no new compare on the typical path).
  - `erf_linear_int` (baseline build all of the band; subnormal x in both): 128-bit integer product with round(c·2^127).
  - `fused::erf_linear_fma` (FMA build, normal x): `fma(x, CH64, x·CL64)·2^-64`.
* Proof: tools/proofs/erf_tiny_f64.py (stdlib Python, ALL CHECKS PASSED); probe of the reference at M0±1 matches.
* Test: crates/boundary/tests/math_fma_paths.rs `erf_tiny_band_rounds_correctly_with_its_flags_in_every_mode` (both
  builds, four modes, every binade + subnormal edges, flags, against the 384-bit path). This is the brief's sweep.

## Done (run 2)
* Static fixed: the FMA intrinsic import in `erf_linear_fma` got its `ARCH(x86_64)` tag; the pending record had `tests`
  after `body` (body must be last).
* Plants file: `int-sign` used `lit:` on text containing ` | ` (the field separator); now a `perl:` plant with `\|`.
* percall spec validation/percall/libm_erf_tiny.c: new row `erf_typ` (erf on [-3, 3]).
* The baseline build's typical erf was +1.0 instruction (146.1 -> 147.1, SSE2 count): the inlined integer path changed
  the register assignment (one extra `movdqa` before the first compare; same compares). Fix (0ccb708ac): the baseline
  build calls the band through `erf_linear_cold` (`#[cold] #[inline(never)]`); the FMA build is unchanged.

## Counts (shiki, Haswell-EP; instructions per call; base = merge-base d1b3a596 of next)
| row | build | glibc | base | new |
|---|---|---|---|---|
| erf_tiny | FMA (default) | 35.0 | 232.0 | 51.7 |
| erf_sub | FMA (default) | 44.6 | 265.9 | 77.0 |
| erf_typ | FMA (default) | 144.8 | 119.6 | 117.6 |
| erf_tiny | `glibc.cpu.hwcaps=-FMA` | 35.0 | 230.9 | 85.8 |
| erf_sub | `glibc.cpu.hwcaps=-FMA` | 44.6 | 263.9 | 75.1 |
| erf_typ | `glibc.cpu.hwcaps=-FMA` | 157.0 | 146.1 | 145.1 |
f32 rows (erff/erfcf) unchanged to the decimal. erf_tiny's operands span 2^-1022..2^-28, so ~3% of them lie above the
band (2^-55..2^-28) and take the existing path: that is most of the gap to erf_sub.

## Jobs (shiki; ~/logs/jobs.tsv)
* final head: libm-erf-tiny-f64-2-lint2 (clippy + static, rc 0), libm-erf-tiny-f64-2-gate-b (rc 0, 2438 passed, --base
  origin/next), libm-erf-tiny-f64-2-plants-b (7 plants), libm-erf-tiny-f64-2-percall-b (default),
  libm-erf-tiny-f64-2-percall-sse2-b (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`).
* superseded (head eabfc0851): lint0 (inherited head, static red), lint1 (rc 0), plants (7 CAUGHT), percall (rc 0),
  percall-sse2 (rc 0; showed the +1.0), gate (stopped: head changed).

## Open / not planted
* The band test itself (`ax < LINEAR` -> `ax < 0.0` gives the right values through the cold path; only counts catch it).
* Directed modes keep `fast_special::erf` + `settle` (correct; case F is proven for them too).
* Next step: none for this story; the coordinator assigns D-10.113 to the pending record.

## Notes
* Process slips run 2: one empty local Python heredoc (call ~10) and one `sed -i` on this handoff file (call ~67), both
  against the standing rules; local only, no remote effect.
