# PERF-SPRINTF-CHK-NLINK: __sprintf_chk with ls-style line

Id: PERF-SPRINTF-CHK-NLINK
Kind: code
Priority: 50
Size: S
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 75k

## Rows (instr/op Liella vs glibc, run label parity-rows-1104-counts2/-counts3 on keke, 3 rounds; time = Liella/glibc dynamic ns ratio on the same run)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| sprintf_chk_ls_nlink | 811.4 | 730.5 | 1.11 | 1.19 |
| (snprintf_ls_line, same formatter) | 2395.8 | 2435.2 | 0.98 | 0.77 |

## Hypothesis

The same formatter is ahead on snprintf, so the 81 extra instructions are in the _chk entry: building the stream/limit state for the fortified call (a second pass over the format for the length check, or an unfused buffer-size computation). Time 1.19 is worse than the instruction ratio, so look at store-forwarding / the setup of the string stream before the conversion loop.
(From the instruction counts per call only; no perf annotate was run in this planning story. First step of the story: count with validation/wl_percall.sh and annotate to confirm before editing.)

## The change

Reduce instructions:u per call on the rows above to at most 1.05x glibc, with no row of the same bench getting slower, behaviour unchanged (every path meets the spec; use run-time dispatch for wider paths, test both). Plants for the new code; proof by --compare --counters before/after.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
