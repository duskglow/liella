# handoff: libm-rethink (LIBM-RETHINK, RETHINK story, planning)

**State.** Branch `task/libm-rethink` from `next` dd2c5b88; HEAD = the commit that adds this note (see
`handoff/reports/LIBM-RETHINK.json`). Stopped at the ~150k-context CHECKPOINT (thread_common BUDGET rule) after steps
1-4 of the brief; steps 5-6 remain. Nothing is wired into the product; the branch adds only `handoff/` files.

**Read first.** `handoff/libm-rethink/DESIGN.md` (inputs, measurements, two or more designs per precision class, the
binary64 exp prototype and its results, provisional recommendation). Prototype and probes: `handoff/libm-rethink/proto/`.

**Decisions in force.** D-10.3, D-10.13, D-10.14, D-10.17, D-10.19, D-10.25, D-10.50 (unchanged). No new D-row yet
(step 6 writes `D-NEW` rows; the coordinator reserves the s10 ids).

**Measured (keke, Zen 1).** Liella vs glibc instructions per call: exp 135/70, log 190/66, expf 73/42, expl 1054/87,
sinl 80 262/1119, atanl 160 923/26 (fpatan), expf128 33 153/6673, cexpf 116 554/162, cexp 116 569/239, csqrt 40 967/144.
One-call traces: long double outside exp/log, binary128 and complex run the 384-bit mp route on every call (95-99% in
`mp::Mp`). Prototype exp (quick phase + Ziv test, today's exp as tier 2): 91.8 (SSE2) / 77.0 (FMA) instructions,
fallback 0.49% / 0.009%, 0 mismatches over 9·10^8 checks, measured error inside the proven bound.

**Open questions.** (1) Why f64/f32 rows are slower in IVB time than in instructions (log 2.87x instr, 5.18x time):
timing per variant, step 5. (2) `sqrtl` 3.58x in IVB time at equal instructions (the percall probe only sees gcc's
inline `fsqrt`; the cbench row needs a look). (3) A published fixed-point CR design to cite for 3.3 A / 3.4 A.

**Do not redo.** The §16/§20/§21/§22-24 tuning (saturation register); the step-2 counts and traces (logs on keke:
`~/logs/libm-rethink-{percall,trace}.log`); the prototype accuracy run (`libm-rethink-proto-accuracy`).

**Exact next step (resume).** Step 5: read `crates/math/src/fast64.rs` (exp path, `settle`), `exp_log` reduce,
`wide_dd.rs`, the `mathret` envelope and `complex::n::cexp` by `grep -n` + ranges (files > 40 KB: never whole), write
DESIGN.md §5. Step 6: finish DESIGN.md §6, write D-NEW rows in `plan/07-decisions/s10-mathematics.md`, and code
briefs in `handoff/libm-rethink/<ID>.md` (first: binary64 exp-family quick phase, kernel and wiring split; then
complex cexp/cexpf from real quick phases; then the long double integer kernel). Book an Ivy Bridge and the N100 timing
for the prototype before 19:30 UTC 11-07 if the resume runs before then (else say timing pending).
