# PERF-STDIO-CHAR-FAST: getc / getc_unlocked / putc / putc_unlocked call cost

Id: PERF-STDIO-CHAR-FAST
Kind: code
Priority: 56
Size: M
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 75k

## Rows (instr/op Liella vs glibc, run label parity-rows-1104-counts2/-counts3 on keke, 3 rounds; time = Liella/glibc dynamic ns ratio on the same run)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| getc_unlocked | 43.3 | 15.3 | 2.82 | 1.13 |
| putc_unlocked | 40.2 | 19.1 | 2.10 | 1.05 |
| getc | 50.4 | 27.3 | 1.84 | 1.17 |
| putc | 47.2 | 32.1 | 1.47 | 0.95 |

## Hypothesis

The unlocked pair costs 25 to 28 more instructions than glibc, which is a whole extra layer in front of the buffer-pointer test (probably a function call into a generic reader/writer and a stream-state check before the read-pointer compare). A fast path of "pointer below end, load byte, advance" inline in the exported symbol, with the generic path out of line, should bring the unlocked rows near 15 to 19; the locked rows then add only the lock enter/exit, which is also worth a look (getc locked minus unlocked is 7 vs glibc 12; putc 7 vs 13 - the lock is already cheap).
(From the instruction counts per call only; no perf annotate was run in this planning story. First step of the story: count with validation/wl_percall.sh and annotate to confirm before editing.)

## The change

Reduce instructions:u per call on the rows above to at most 1.05x glibc, with no row of the same bench getting slower, behaviour unchanged (every path meets the spec; use run-time dispatch for wider paths, test both). Plants for the new code; proof by --compare --counters before/after.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
