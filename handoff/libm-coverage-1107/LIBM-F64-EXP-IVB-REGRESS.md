# LIBM-F64-EXP-IVB-REGRESS: find what made Ivy Bridge `exp` 1.86x -> 2.90x between 11-05 and 66b8d3ba

Id: LIBM-F64-EXP-IVB-REGRESS
Kind: code
Priority: 87
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 70 tool calls / 140k

## The change

Row: cbench_libm/exp (uniform [-10, 10]) on Ivy Bridge (E3-1265L v2): 1.86x glibc time at the 11-05 figures
(docs/benchmarks.md, tree a6fc34c2) -> 2.90x at 66b8d3ba (TIMING-SWEEP-LIBM-1107); N100 3.35x (no 11-05 baseline).
The rethink does not replace exp outright: LIBM-F64-EXP-QUICK-WIRE puts a quick phase in front and keeps the envelope
and today's path as tier 2, so a cause in the entry, dispatch or envelope survives it. This story runs before or
beside the WIRE story and hands its finding to it.

Evidence that narrows it: exp_small (the same function on [-0.5, 0.5]) was 2.92x on 11-05 and is 2.91x now, so the
11-05 exp figure was LOWER than its own small-argument row; either a path taken for |x| > 0.5 got slower, or the 11-05
exp figure was itself an outlier. LIBM-RETHINK-2's own harness measures 2.96x for exp today (DESIGN.md 4.1), so today's
figure is solid. Other 11-05 -> now rows: exp_upward 12.39 -> 11.22, exp_tiny 7.87 -> 3.69, exp2 5.38 -> 5.49, expf
2.10 -> 2.11 (no shared regression visible).

Candidate libm landings between a6fc34c2 and 66b8d3ba (`git log --oneline --first-parent a6fc34c2..66b8d3ba --
crates/math crates/boundary/src/hot crates/boundary/src/mathret.rs crates/boundary/src/dispatch.rs crates/rt/src/fpu.rs`):
libm-f64-fast-kernels-3 (83d18d49e, D-10.50 load-time FMA decision: changes how the SSE2 build Ivy Bridge runs is
reached), libm-f64-explogpow-tier (dc65f7614), libm-tables-per-family-2 (c6e5db9d3, table layout), libm-hot-per-family-2
(96528aa7f), libm-kernel-index (a2ac728a8), libm-mp-core-speed-2 (984a2ddc0), libm-wide-dd-tier-2 (e7d3bfd6e). Read
their handoffs (handoff/libm-*.md, LIBM-*-plants.txt), never other people's diffs beyond what the brief rules allow.

Method: (1) counts first: `wl_percall` exp and exp_small cases at a6fc34c2 and at 66b8d3ba, SSE2 build forced
(`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`, what Ivy Bridge runs) and FMA build; prime both revisions with
validation/wl_prime_base.sh. (2) If the counts moved, bisect the candidate merges by count (each a cached product).
(3) If counts did not move, the cause is per-instruction (alignment, a store-forward stall, a table moved off a cache
line, a `#[target_feature]` call boundary): booked Ivy Bridge timing of exp and exp_small at a6fc34c2 and 66b8d3ba
(staged trees, `CBENCH_ROWS=exp,exp_small`, 7 rounds), then the candidate merges; also re-time a6fc34c2 alone twice to
settle whether 1.86 was an outlier (if it reproduces at 2.9, report "no regression: the 11-05 figure was an outlier"
with the numbers and stop). (4) Fix the cause if it is local and does not conflict with LIBM-F64-EXP-QUICK-WIRE;
otherwise write the finding into that story's handoff.

## Tests and plants
Existing exp accuracy tests: `crates/math/tests/fast64.rs` (432 hard operands), `elem_products.rs`,
`math_double_fast_entry_sweep`. A fix gets plants on the changed code (CAUGHT with msg:) and no accuracy change.

## JOBS
counts at both revisions (labels named), booked Ivy Bridge timing of exp/exp_small at both revisions (and N100 at the
fix), clippy + static + direct gate + plants if code changes.

Hand-back first line: `COMPLETE: exp IVB regression caused by <landing|outlier>, <fixed|handed to WIRE>` or INCOMPLETE:.
