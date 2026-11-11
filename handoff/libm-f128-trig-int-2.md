# handoff: libm-f128-trig-int-2 (LIBM-F128-TRIG-INT-2) - INCOMPLETE, do not land as it stands

**State.** Branch `task/libm-f128-trig-int-2` from origin/next 7fcb6acda. Role keke:~/w/r5. Code commits: d89cb27be
(tan/sincos tier, quotient, tests, bench and percall rows, plants file), 0608873b0 (type alias, D-10.91 record),
b7cdfe78e (record escape), ada683d87 (sin/cos kernel restored to its landed form), 5ca4f853a and d05b6e52a (two
inlining experiments, both reverted by the commit after this note's predecessor; code now equals ada683d87).

**What it is.** `crates/math/src/wide_int/quad.rs`: `both` runs D-10.85's reduction and both polynomials once;
`sin_and_cos` (from `wide::sincos`, 113-bit format only) rounds sin and cos each, and an undecided one alone takes
the 384-bit route; `tan` (from `wide_int::arc::tan`) divides them with `div3` (one `divsd`, a Newton step on the
reciprocal, one exact residual correction: proved 2^-162.2, measured worst 2^-166.0). Proof steps 6 and 7 in the
module doc: tan within 2^-155.6 (the rounding test assumes 2^-152). Decision record D-10.91 (records/decisions/D-10).

**Results.**
* Unit tests (`cargo test -p liella-math --lib wide_int::quad`): 4 passed; 24 000 calls (sin, cos, tan, sincos),
  0 undecided, worst 2^-159.4; `quotient_meets_its_bound` worst 2^-166.0 (jobs unit5 on b7cdfe78e; clippy and
  static clean in stage2 on ada683d87).
* Sweep `libm-f128-trig-int-2-sweep` (b7cdfe78e, release, `tier_agrees_with_the_wide_route_long`, now covering tan
  and sincos): 10^7 operands, 2x10^8 calls in four modes, 0 undecided, 0 refused, all equal to the 384-bit route,
  worst 2^-158.26; near multiples at all 16 384 exponents: 327 680 calls, 131 072 undecided by design, worst
  2^-166.22, closest |f| 2^-124.
* Plants `libm-f128-trig-int-2-plants2` (b7cdfe78e): 6 CAUGHT (quotient correction limb dropped, half Newton step,
  residual column dropped, tan sign, sincos pick, rounding test skipped), 0 WARN-RC-ONLY/NO-MSG (one WARN-DEV-SHM
  notice from the tool about /dev/shm).
* Counts (wl_percall, `validation/percall/libm_f128_trig.c`, instructions:u, glibc / base / new):
  tanf128 8173 / 89 475 / 1749 (0.21x); tanf128_1e300 8491 / 77 574 / 1744; sincosf128 8756 / 2439 / 1831 (0.21x).
  **But sinf128/cosf128 7251 / 1257 / 1332: +74 per call (percall3 on ada683d87, percall5 on d05b6e52a).** The first
  version (d89cb27be, sin/cos through the shared helpers) measured 1358; restoring the kernel's source (ada683d87)
  gave 1332; `#[inline(always)]` on `reduce` gave 1374 (5ca4f853a); on `word` 1332 (d05b6e52a). Both reverted.
* Timing (booked, staged cbench_libm `--compare --rows`, b7cdfe78e build, i.e. the 1358 sin/cos), glibc / Liella ns:
  Ivy Bridge (kotori) sinf128 1830.7 / 404.6 (0.22), cosf128 1809.3 / 399.0 (0.22), sincosf128 2238.5 / 526.9
  (0.24), tanf128 2263.5 / 549.4 (0.24); N100 (nozomi) sinf128 1393.0 / 326.3 (0.23), cosf128 1383.1 / 322.2
  (0.23), sincosf128 1722.4 / 419.5 (0.24), tanf128 1667.5 / 414.7 (0.25). The sweep table of 11-10 had sin/cos at
  about 20x and tan 16.8x on Ivy Bridge: the landed tier plus this one put all four at about 0.22-0.25x.

**Why INCOMPLETE.** ALWAYS FORWARD: sinf128/cosf128 are 74 instructions slower than on next although their kernel
source is unchanged. Two guesses (reduce, word out of line) were wrong; the cause is not known. Candidates: other
quad.rs inlining (`round`, `sin_poly`/`cos_poly`, `mulhi3` now have more callers), or `wide.rs` (the new sincos
branch gives `trig_special`, `cubic_tiny`, `approx`, `sin_cos_of` more F128 callers).

**Next step (LIBM-F128-TRIG-INT-3).** Profile first: `validation/wl_trace.sh` (or disassemble ~/wl/products/<sha>/libm.a)
for sinf128 at 7fcb6acda and ada683d87 and find the extra 74 instructions; fix them (sin/cos must return to 1257 or
below), rerun percall, the direct-tier gate (`cargo xtask affected --run --base origin/next`; not run in this story),
plants (`handoff/LIBM-F128-TRIG-INT-2-plants.txt`) and the sweep on the final head, re-time if the code moves. Then
the inverse functions (atanf128, atan2f128, asinf128, acosf128) on three limbs, `div3` is ready for their quotient.

**Do not redo.** div3's design and proof; the Python model of div3 (it caught the 128-bit epsilon bug); the bench
row `sincosf128` (cbench_libm, budgets.tsv) and the percall rows.
