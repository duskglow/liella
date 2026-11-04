# PERF-CTYPE-LANGINFO-ACCESS: __ctype_b_loc / isalpha / nl_langinfo(_l) / wcwidth accessors

Id: PERF-CTYPE-LANGINFO-ACCESS
Kind: code
Priority: 52
Size: M
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 75k

## Rows (instr/op Liella vs glibc, run label parity-rows-1104-counts2/-counts3 on keke, 3 rounds; time = Liella/glibc dynamic ns ratio on the same run)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| nl_langinfo_l_en_us | 44.0 | 28.0 | 1.57 | 1.22 |
| ctype_b_loc | 14.0 | 10.0 | 1.40 | 0.83 |
| isalpha_c | 22.0 | 16.0 | 1.37 | 0.99 |
| wcwidth_utf8 | 54.0 | 43.0 | 1.26 | 0.95 |
| nl_langinfo_en_us | 35.0 | 31.0 | 1.13 | 1.14 |

## Hypothesis

Each is a handful of instructions over, so the cause is the locale lookup: __ctype_b_loc and the thread locale pointer take 4 more instructions (an extra indirection or a TLS access through a helper instead of an initial-exec load), and nl_langinfo(_l) decodes item to category/index with a longer path than needed (a table lookup indexed by item should take about 10). Excluded on purpose: iswalpha, iswctype, towupper (open PERF-WCTYPE stories).
(From the instruction counts per call only; no perf annotate was run in this planning story. First step of the story: count with validation/wl_percall.sh and annotate to confirm before editing.)

## The change

Reduce instructions:u per call on the rows above to at most 1.05x glibc, with no row of the same bench getting slower, behaviour unchanged (every path meets the spec; use run-time dispatch for wider paths, test both). Plants for the new code; proof by --compare --counters before/after.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
