# V-4 bench rerun (PRE14-BENCH-RERUN)

State: branch task/pre14-bench-rerun. docs/benchmarks.md rewritten from a full bench of next a6fc34c2.

Measured: both timing hosts booked (nico #25, kotori #26, 240 min), full `xtask bench --no-build --counters` of prebuilt
products (built on shiki sh1, job pre14-bench-rerun-build, rc 0). nico (E3-1220L v2) 4571 s, kotori (E3-1265L v2) 4221 s.
Both Ivy Bridge (no AVX2): no AVX2 host was measured. Logs/tsv: nico/kotori ~/work/PRE14-BENCH-RERUN/out-<host>/.
kotori: 13 rows over their cycle limits (limits from 5236e77 calibration), listed in its bench.log; nico has no limits.
The timing runs ran under ~/bin/timing (label timing:pre14-bench-rerun-bench-<host>); they have no jobs.tsv row.

Summary (counters): 542 counted rows, vs glibc 235 faster / 63 parity / 244 slower; 165 of the slower are libm (median
instr ratio 9.7 f64, 18 f32, 504 long double). Without libm: 234 / 60 / 79. Against the 10-23 counted record (same CPU
class, 214 joined rows): 43 better / 163 same / 8 worse (+-3%). Against 5cc49dc by time (Haswell, different class,
advisory): 122 lower / 24 same / 13 higher of 159.
No 5cc49dc instruction counts exist (page was time only; commit not rebuilt).

Not to redo: the page generator and analysis scripts are in scratch only; rerun needs the staged tree (see section 6).
Next step: owner decides whether a 5cc49dc counted build on an Ivy host is wanted, and an AVX2 timing run.
Open rows for owners: iswalpha*/iswctype (+31..35% instr since 10-23), gettext_language_* (+11..18%), libm gap (4a).
Plan rows: plan/06-phases/16-validation-and-optimization-before-phase.md (V-4); docs/saturation/.
