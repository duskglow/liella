# MATH-M6-F32-BENCH-2-R: port the float math per-call spec and one behaviour paragraph that never reached next

Id: MATH-M6-F32-BENCH-2-R
Kind: code
Priority: 50
Size: S
Host: -
Model: sonnet
Effort: medium
BUDGET: 70 tool calls / 140k

## The change

Everything else in MATH-M6-F32-BENCH-2 and MATH-M6-FPENV-HONOR-4 is already on next (HELD-REDS-TRIAGE, 2078/2123 and
2081/2118 added lines present). Two small pieces are not:

1. `validation/percall/math_f32.c` (40 lines), from `origin/task/math-m6-f32-bench-2` (the single commit "Carry
   task/math-m6-f32-bench-2 from liella-historic"): the wl_percall float set (every fast32 row, typical and
   reduction-heavy ranges, sqrtf as the floor). Port it with `git show origin/task/math-m6-f32-bench-2:validation/percall/math_f32.c`.
   Check it still builds and runs against the current percall.h and harness (`wl_percall.sh --spec`), that the rows
   (sinf, cosf, sincosf, tanf, expf, logf, log1pf, expm1f, cbrtf, hypotf, powf, powf_int, sqrtf) exist on next, and
   fix function names if they moved. Its header says "no spec clause". Run percall for it and report instruction counts.
2. A 7-line paragraph for `spec/behaviour/libm-exact.md` from `origin/task/math-m6-fpenv-honor-4` ("The caller's SSE
   control word is honoured, flush-to-zero and denormals-are-zero included ... D-10.22 ..."). Add it only if
   libm-exact.md on next does not already say this (spec/behaviour/math-ftz-daz.md and D-10.22 do); if the paragraph
   duplicates them, link to math-ftz-daz.md instead of copying.

Run `cargo xtask static` (spec traceability) and clippy. No library code changes. Jobs: the percall run.
