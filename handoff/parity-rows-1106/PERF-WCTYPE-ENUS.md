# PERF-WCTYPE-ENUS: parity rows (instructions:u), from the PARITY-ROWS-1106 round

Id: PERF-WCTYPE-ENUS
Kind: code
Priority: 60
Size: M
Host: -
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 70 tool calls / 140k

## Rows

Counter run: label parity-rows-1106-counts on a Broadwell-EP (Xeon E5-2699 v4) counter host, `cargo xtask bench --compare --counters --runs 3`, origin/next 412d6fc7; time = Liella/glibc dynamic ns ratio of the same run; per-call profile: label parity-rows-1106-percall2 (validation/wl_percall.sh, instructions:u per call, deterministic, perf annotate not used). Raw: bench-cbench_opt.out is opt.out, others.out in this directory.

| row | instr ratio | time ratio | percall (Liella / glibc) |
|---|---:|---:|---|
| iswctype_en_us | 2.42 | 2.39 | iswctype_cjk 105 / 46 (2.28); iswctype_ascii 59 / 47 |
| iswalpha_l_en_us | 2.32 | 1.82 | iswalpha_l_cjk 121 / 50 (2.42) |
| iswalpha_en_us | 2.04 | 1.28 | iswalpha_ascii 55 / 22 (2.50); iswalpha_latin1 55 / 53 (1.04) |
| towupper_l_en_us | 1.74 | 1.30 | towupper_l_latin1 80 / 42 (1.91); towupper_ascii 60 / 43 (1.40) |
| iswalpha_utf8 / towupper_utf8 / wcwidth_utf8 | 1.39 / 1.32 / 1.16 | 0.93 / 0.77 / 0.78 | time already faster |

## Hypothesis (from the per-call profile)

The gap is a fixed per-call overhead, not a table walk: iswalpha_ascii costs 55 instructions where glibc spends 22, while iswalpha_latin1 (a table hit further in) is 55 vs 53. So Liella spends about 33 instructions before it reaches its lookup (locale fetch, class dispatch, bounds) and the lookup itself is on par. The same 30-40 instruction shape shows in towupper_ascii (60 vs 43) and iswctype_ascii (59 vs 47). The CJK cases (iswalpha_l_cjk 121 vs 50, towlower_cjk 102 vs 41) add a second, larger cost on the multi-level table path. The 1105 wctype stories landed and the rows are still above 1.05: what they got is the utf8 variants (time 0.77-0.93) and the latin1 rows; what is left is the en_US (locale-object) entry and the ASCII/CJK fast paths. First step: percall --cases iswalpha_ascii,iswalpha_l_cjk with a call-tree (validation/wl_trace.sh) to see where the 33 fixed instructions go.

## The change

Cut the fixed entry cost of the isw*/tow*/iswctype calls (and their _l forms) under a UTF-8 locale so the ASCII path is at most about glibc's instruction count, then shorten the CJK path. Target: all five rows above at most 1.05x glibc in instructions and at most 1.0x in time. Behaviour unchanged for every code point (test the whole range against the current result), no change to the locale object layout. Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both). Plants for new or changed code; proof by `cargo xtask bench --compare --counters` before/after on the rows above, no row of the same bench slower.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/. Plants run through `cargo xtask plants` only.

JOBS: gate, plants (`cargo xtask plants`), counts (before and after), percall
