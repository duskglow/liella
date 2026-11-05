# MATH-M6-F32-BENCH-2-R

State: branch task/math-m6-f32-bench-2-r, code head 858fb6f5 (plus the report commit). No library code changes.

Done:
* `validation/percall/math_f32.c` ported unchanged from the side branch (no spec clause). It builds and runs against the
  current percall.h and harness; all 17 rows check "ok"; every function name exists on next.
* `spec/behaviour/libm-exact.md`: the FTZ/DAZ paragraph was NOT on next, so it is added (D-10.22), with a pointer to
  `math-ftz-daz.md` for the software families instead of repeating that table.
* `validation/wl_percall_run.sh`: the Liella variant now uses the product tree's own `scripts/liella-cc` first and
  `~/liella-test/scripts/liella-cc` only as a fallback. Cause: the mei host has an old (Oct 4) `~/liella-test` copy
  without `-lm` handling, so every math spec (hypotf.c too) failed to link with undefined sinf etc.

Percall (instructions:u per call, mei, base = new because no library change):
row: glibc / Liella  (ratio)
sincosf_red 92.0/1846.8 (20.1); tanf_red 136.9/1020.4 (7.45); cosf_red 74.0/946.3 (12.8); sinf_red 74.0/945.3 (12.8);
powf_int 73.1/326.2 (4.46); sincosf_typ 59.1/240.0 (4.06); powf_typ 78.0/254.2 (3.26); sinf_typ 43.2/152.0 (3.52);
cosf_typ 43.6/152.0 (3.49); log1pf_typ 88.0/153.3 (1.74); tanf_typ 106.5/160.0 (1.50); expm1f_typ 91.7/142.0 (1.55);
logf_typ 46.0/80.0 (1.74); expf_typ 42.0/73.0 (1.74); cbrtf_typ 139.0/158.8 (1.14); hypotf_typ 37.0/50.0 (1.35);
sqrtf_floor 10.0/10.0 (1.00).

Not redo: the port, the paragraph. Next step: none for this story. The reduction rows (12 to 20x glibc) and the
sin/cos/powf typical rows are the largest float-math gaps in instructions; a follow-up story could target them.
Jobs: math-m6-f32-bench-2-r-lint2 (static + clippy, rc 0), math-m6-f32-bench-2-r-percall2 (rc 0).

## Continuation MATH-M6-F32-BENCH-2-R-2
Merged origin/next. The only conflict was validation/wl_percall_run.sh: next had made the same change (prefer the cached tree liella-cc), so the merge keeps that code and one comment with both reasons. Reran on kanon at e6c51e6d5 with --base none (no library change; the old base was not cached on that host): the 17 float percall rows are the same as above, all check ok. Gate (affected) rc 0, clippy -D warnings and cargo xtask static clean.
Jobs: math-m6-f32-bench-2-r-2-gate, math-m6-f32-bench-2-r-2-percall3 (earlier: -percall rc 2 base not cached; -percall2 rc 1 cargo not on PATH in job; both superseded).
