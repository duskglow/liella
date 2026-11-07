# TIMING-COV-WCTYPE-L: iswctype and the _l classification entries cost 1.3-2.1x glibc on both classes

Id: TIMING-COV-WCTYPE-L
Kind: code
Priority: 66
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 calls / 160k
Stacks-on: none

## The change
Bring `iswctype`, `iswalpha_l`, `nl_langinfo_l`, `towupper_l` (and `iswalpha` in C.UTF-8 on the N100) to glibc's
cost. PERF-WCTYPE-ENUS-2 (landed) fixed plain `iswalpha` in en_US (now 0.88 / 0.99); its sibling entries were left.

## Rows (1106E; instr / time IVB, instr / time N100)
iswctype_en_us 1.768 / 2.090, 1.767 / 1.840; iswalpha_l_en_us 1.489 / 1.330, 1.489 / 1.448; nl_langinfo_l_en_us
1.250 / 1.221 (IVB; N100 0.801 faster); nl_langinfo_en_us 0.839 / 1.100 (IVB only); towupper_l_en_us 1.000 / 1.067
(IVB only); iswalpha_utf8 1.347 / 0.955 IVB, 1.347 / 1.061 N100.

## Evidence (build host, region counters per call, counters-mei.txt)
iswctype_en_us 76 vs 43 instructions, 20.6 vs 11.5 cycles; iswalpha_l_en_us 70.1 vs 47.0, 22.7 vs 13.4;
nl_langinfo_l 35 vs 28, 11.2 vs 10.2; iswalpha_utf8 66 vs 49, 17.2 vs 17.7; towupper_l 39 vs 39, 11.1 vs 12.9.
Liella samples (whole group): __iswalpha_l 21.4%, __iswctype 19.3%, `liella::ctype::wclasses` 11.7% (a separate
function, not inlined), __towupper_l 8.6%, __nl_langinfo_l 6.8%, iswalpha 4.3%.

## Hypothesis (to verify in the code first)
`__iswctype` and `__iswalpha_l` reach the class table through the out-of-line `ctype::wclasses` (and a locale-handle
resolution) that the fixed plain `iswalpha` path avoids. Read D-rows of s05 (plan/07-decisions/s05-character-
classification.md) and PERF-WCTYPE-ENUS-2's handoff; give the _l and iswctype entries the same direct path. The
IVB-only nl_langinfo / towupper_l time gaps at equal or lower instruction counts are entry/layout effects: check
after the main change, do not chase them first.

## Tests and plants
wctype/wctrans/nl_langinfo tests on the direct tier (spec s05 clauses); plants on the changed entries (a wrong
class bit, a wrong locale handle).

## Rows to time
cbench_locale: iswctype_en_us, iswalpha_l_en_us, iswalpha_en_us, iswalpha_utf8, towupper_l_en_us, towupper*,
nl_langinfo_en_us, nl_langinfo_l_en_us, and the rest of the group (no row slower).
## JOBS
Gate: `cargo xtask clippy`, `cargo xtask static`, then the direct tier (`cargo xtask affected --run --base origin/next`)
through jobrun on your role. Plants: `cargo xtask plants <file>` on every changed function (msg: regex per plant).
Counts before/after on base and head (region counters or the probe, both sides printed). Timing: book an Ivy Bridge
and an N100 host (`ops timing-book`), stage once with `cargo xtask bench --stage`, run the rows below base vs head
vs glibc interleaved (>= 3 rounds), cancel the bookings when done. Keep only a change that makes no measured row slower.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
