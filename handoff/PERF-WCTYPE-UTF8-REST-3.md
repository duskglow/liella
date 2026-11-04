# PERF-WCTYPE-UTF8-REST-3 (branch task/perf-wctype-utf8-rest-3)

State: fix (a) built. Disassembly first: the old `LocaleView::wtrans` kept 7 pushes/6 pops before the tag test.
With the file-backed arm in `#[inline(never)] wtrans_wide`, `isw_wide`, `width_wide`, the built-in arm inlines into
`towupper` (no prologue). Counts (sumire, `cbench_locale`, default level): towupper_utf8 83.0 -> 53.0 instr (glibc 40.0).
iswalpha_en_us 66.0 / wcwidth_utf8 54.0 / iswalpha_utf8 71.1 were measured BEFORE the isw/width change (unchanged
rows); the after-counts for them are in sumire ~/work/perf-wctype-utf8-rest-3-fin-d.out and -fin-s.out (job gate3).
Not verified: those after-counts, the -AVX2 level, the gate result (gate3 was still running at hand-back).
Plants (plants3, 3 caught): wtrans file arm (upper flag), isw Latin class mask, wtrans built-in arm. No plant for
`width_wide` Latin arm: it SURVIVED the direct tests (no test of wcwidth on a file-backed Latin-1 char): gap, INFORMATIONAL.
Remaining to 40: ~22 in ctype::wtrans decode + 3-level table walk (idea (c): flat delta table), not tried.
Decision: keep only if fin counts show no row slower; a row slower -> revert isw/width part only.
