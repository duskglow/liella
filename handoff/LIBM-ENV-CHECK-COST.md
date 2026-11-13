# LIBM-ENV-CHECK-COST: the libm entry's MXCSR check (#4872)

State: branch `task/libm-env-check-cost`, code head 32448ffa7 (report commits on top). Decision D-10.101 (amends D-10.71,
D-10.22). Plants: `handoff/LIBM-ENV-CHECK-COST-plants.txt`. Percall spec: `validation/percall/libm_env_check.c`.

## 1. Measured cost of the check (before)

Loop probe (`~/work/LIBM-ENV-CHECK-COST/mxcost.c` on each host; job `libm-env-check-cost-mxcost-<host>`), TSC ticks
per iteration, best of 7, throughput loop (independent iterations) unless marked chain:

| | Zen 1 (keke) | Broadwell-EP (kanon) | Skylake-SP (mei) |
|---|---|---|---|
| empty loop | 3.08 | 3.03 | 4.02 |
| check only (stmxcsr, load, mask, cmp, branch) | 23.81 | 4.19 | 4.03 |
| sqrtsd / + check | 8.20 / 24.53 | 7.65 / 7.58 | 6.02 / 6.02 |
| sqrtss / + check | 5.12 / 24.31 | 4.16 / 5.06 | 4.08 / 5.03 |
| sqrtsd chain / + check | 23.53 / 23.58 | 23.43 / 23.22 | 22.09 / 22.07 |
| glibc sqrt, sqrtf, expf, logf, tanhf | 11.3, 12.2, 12.2, 12.4, 14.6 | 9.1, 9.1, 13.9, 14.4, 13.2 | 9.0, 9.0, 14.3, 15.1, 14.7 |
| glibc exp, log, sinf, sin | 21.0, 20.0, 22.5, 47.5 | 22.8, 20.2, 22.4, 50.8 | 22.1, 21.1, 22.5, 40.7 |

So on Zen 1 the check is ~21 ticks of reciprocal throughput: the throughput floor of every short entry that has it
(sqrt, sqrtf, the elem32_fast float functions sinf/cosf/tanf/sinhf/coshf/tanhf/asinhf/acoshf/atanhf, sincosf, every
elem_fast double function). In a latency-bound chain it is hidden; on the Intel classes it is 0-1 tick. expf, exp2f,
exp10f, logf, log2f, log10f (one-pass entries, D-10.63) already read nothing.

## 2. Design (D-10.101)

* (a) per-thread "environment is default" byte: REJECTED. MXCSR is user state written outside the library: inline
  ldmxcsr, setcontext, a handler editing the saved frame before sigreturn, and a handler's fesetround(FE_TONEAREST)
  landing between an interrupted fesetround's flag store and its ldmxcsr. A stale byte gives round-to-nearest results
  in another mode or runs arithmetic under an unmasked trap. No protocol keeps it exact, so no plant can make it pass.
* (c) arithmetic mode detection (one addpd whose lanes round differently per direction, a subnormal lane for FTZ/DAZ):
  REJECTED: cannot see the exception masks, and raises inexact in the caller's word.
* (b) read only where the result depends on the mode: ADOPTED for sqrt/sqrtf, whose instruction is the specified
  operation under every word. Black-box probe (`sqrttrap.c`, probe f9aa46dc8b82): the reference's sqrt/sqrtf trap in
  the instruction for invalid (-1, -inf), inexact (2) and denormal-operand (subnormal) when unmasked, errno untouched at
  the trap; with invalid unmasked and DAZ, sqrt(-subnormal) = -0, no trap. The old Liella fallback differed in 4 cases
  (no DE trap or flag; an invalid trap for sqrt(-sub) under DAZ); now identical.
* The root is pinned (`.pin()`, no instruction) so EDOM is stored after the instruction: without it the compiler
  derived x < 0 from x and stored EDOM before the invalid trap (caught by the new conformance test, then fixed).

## 3. Tests

* `math_sqrt_runs_in_place_under_every_mask` (renamed from `math_sqrt_stays_out_of_place_under_an_unmasked_trap`; the
  old one asserted the software fallback under an unmasked DE trap).
* `conformance/c/s10_sqrt_env.c`: fesetround x4, inline ldmxcsr x4 directions x FTZ/DAZ combinations (subnormal
  operands), a new thread created in each direction (inherits; its own change stays its own), a signal handler that
  changes direction while the interrupted code is in FE_UPWARD, unmasked traps with errno at the trap.

## 4. Results

See handoff/reports/LIBM-ENV-CHECK-COST.json and the job logs on kanon: `libm-env-check-cost-check3` (clippy, static,
records, direct tests: green), `libm-env-check-cost-gate`, `libm-env-check-cost-plants`,
`libm-env-check-cost-percall` (instructions:u) and `libm-env-check-cost-percall-cycles` (cycles:u, Broadwell).

## Open / next

* Zen 1 time of the new sqrt/sqrtf entries against primed next (Liella-linked loop on keke) is not measured in this
  run; the model above predicts sqrt ~24 -> ~8-11 ticks and sqrtf ~24 -> ~5-12 ticks per call in a throughput loop.
* Follow-up briefs (not written as files): (1) the float elem32_fast family (sinf, cosf, tanf, sincosf, the
  hyperbolics) to one-pass entries (D-10.63 method: double evaluation right in every direction, refusal before any
  operation that could raise more than inexact) - removes the read on Zen 1, per-function error analysis; (2) the
  double elem_fast entries keep the read: their results depend on the mode; only a directed-rounding error analysis of
  each kernel (double-double steps are not exact outside round to nearest) could remove it.
* INFORMATIONAL: the old software fallback trapped with invalid for sqrt(-subnormal) under DAZ with invalid unmasked;
  the switched path of the other envelopes (elem_switched) computes in the default word, so it does not share that
  path, but it was not probed with unmasked traps here.
