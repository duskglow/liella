# LIBM-F32-ONEPASS-REST: handoff (binary32 functions onto the one-pass `float` path)

State: branch `task/libm-f32-onepass-rest-3` (run 3; stacks on run 2's 07ceda237, origin/next 4070276c5 merged in run 2;
no merge this run). Role mei:~/w/s3. Decisions in force: D-10.72 (extends D-10.63 under D-10.67, D-10.68). Plan rows to
read: D-10.63, D-10.67, D-10.68, D-10.72. Reports: handoff/reports/LIBM-F32-ONEPASS-REST{,-2,-3}.json.

## Done (all runs)
* Runs 1-2: exp10f, logf, log2f, log10f on the one-pass path (`fast32::exp10_onepass_any`, `fast32::log_onepass`),
  exhaustive 2^32 x 4 directions 0 mismatches each (exp10f re-swept after run 2's rotate-compare refusal:
  shiki:f32r2-sweep rc 0, 0 mismatches). `cargo xtask so` writes a source digest; `static` checks only a product of
  this tree (17587fa67).
* Run 3 (93732e3f0): sinf, cosf, tanf (`fast32::trig_core`, also used by sincosf, the float complex and Bessel paths),
  asinhf (`asinh_core_with`) and acoshf (`acosh_core_with`) test their fast range on the bits (one subtract + unsigned
  compare) and build the `double` operand from the bits (exponent + 896, fraction << 29, sign copied): exactly
  `f64::from(x)`, same refusal sets, no `cvtss2sd`. The partial-write ALLOW list is EMPTY: `cargo xtask so` on mei
  reads "791 exports walked, 0 findings (0 allowed)". `hyp_arg` itself is unchanged (sinhf/coshf/tanhf/atanhf keep
  it; they had no finding). D-10.72's body records the extension.
* Fast32 unit tests (17) pass on the run-3 code (mei, libm-f32-onepass-rest-3-regen2 log).

## The page-fault red of run 2 (gate f32r2-gate on shiki: coreutils-shaped tool 59 > bound 58)
* On mei, with run 3's code: tool 57, locale 53, startup 16, fork child 7 (all within their bounds; five equal runs,
  libm-f32-onepass-rest-3-regen2). The gate on the final head (below) is the proof on the CI host class.
* Not fixed by a list regen: none was needed on mei, so `crates/libc-so/hot-text.list` is unchanged. (The planned
  regen did not run: the synced `tools/hottext/regen.sh` has no exec bit in the role; call it with `bash`.)
* The layout stays fragile: fast products use 16 codegen units with thin LTO, so a change anywhere in liella_boundary
  moves generic instances (`ret::at::<i8>` etc.) between inlined and out-of-line, and the tool count moves by 1-3.
  shiki read 59 on run 2's head vs 56 for its origin/next base; this head was not measured on shiki.

## Jobs of run 3 (mei; on the code of 93732e3f0 + the record fix; head in the report json)
* libm-f32-onepass-rest-3-gate: `cargo xtask affected --run --base origin/next` (escalated to FULL: xtask changed): rc 0,
  32 suites ok, 11426 passed, 0 failed; conformance 126 passed (includes hot_text_products on mei).
* libm-f32-onepass-rest-3-plants: rc 0, 6 CAUGHT, 0 SURVIVED, 0 WARN (handoff/LIBM-F32-ONEPASS-REST-3-plants.txt).
* libm-f32-onepass-rest-3-sweep: exhaustive sinf, cosf, tanf, asinhf, acoshf (2^32 x 4 each), after the gate.
* libm-f32-onepass-rest-3-counts: cbench_libm --compare --counters, FMA and SSE2 builds, after the sweep. Base counts
  (origin/next 4070276c5, same merge base): `=== origin/next` section of shiki:~/logs/f32r2-counts.log (run 2).
  Run 2's branch figures (instructions per call, Liella / glibc, FMA): sinf 85.2 / 42.0, cosf 85.2 / 42.1, tanf 93.4 / 106.3.

## Not done / open
* Timing: run 3 booked none (budget). The trig, asinhf and acoshf change alters the instruction mix (integer ops for
  one `cvtss2sd`), so it is a timing-exception row: time sinf, cosf, tanf, asinhf, acoshf on Ivy Bridge (nico or
  kotori, whichever queue is shorter) and the N100 (nozomi), Liella vs glibc interleaved. N100 so far (#4308): exp10f
  1.59, logf 1.60, log2f 1.43, log10f 0.82, expf 1.66 glibc.
* powf (IVB 4.47x; instructions per call Liella 249.7 / glibc 76.1 in run 2's counts) not started.

## Next (in order)
1. Read the four jobs above; on a red, fix the cause (plants: any SURVIVED or WARN line).
2. Time the five run-3 functions on both timing hosts (book for the job's length; `ops timing-cancel <id>` after).
3. powf: many exact cases; refuse on the bits or prove the kernel exact there. Start from `onepass_scaled` and
   `log_onepass`'s bit split.

## Do not redo
* The exhaustive sweeps of exp10f/logf/log2f/log10f (code unchanged). The partial-write CI red (cause fixed in run 2).
* The hot-text regen for this head on mei (not needed: 57 <= 58).
