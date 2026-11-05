# PERF-SPRINTF-CHK-LS: parity rows (instructions:u), from the PARITY-ROWS-1106 round

Id: PERF-SPRINTF-CHK-LS
Kind: code
Priority: 57
Size: S
Host: -
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 70 tool calls / 140k

## Rows

Counter run: label parity-rows-1106-counts on a Broadwell-EP (Xeon E5-2699 v4) counter host, `cargo xtask bench --compare --counters --runs 3`, origin/next 412d6fc7; time = Liella/glibc dynamic ns ratio of the same run; per-call profile: label parity-rows-1106-percall2 (validation/wl_percall.sh, instructions:u per call, deterministic, perf annotate not used). Raw: bench-cbench_opt.out is opt.out, others.out in this directory.

| row | instr ratio | time ratio | percall (Liella / glibc) |
|---|---:|---:|---|
| sprintf_chk_ls_nlink | 1.18 | 1.16 | spr_ls 904 / 726 (1.24); chk_ls 907 / 744 (1.22) |

## Hypothesis (from the per-call profile)

Control rows in the same spec are at or below parity: chk_lu 437 / 536 (0.82) and chk_cpc 372 / 365 (1.02). So the integer and %c conversions are fine and the excess (about 165-180 instructions per call) sits in the %s / width path of the long listing format ("%s %*u ..." with a padded link count and a name), i.e. string conversion with width/precision and padding, or the per-call parse of a longer format. Hypothesis to confirm with wl_trace: padding emitted byte by byte, or the string length taken twice (strlen for precision, then copy). First step: percall --cases spr_ls with the call tree.

## The change

Remove the extra per-call work in the %s / padded-width path of sprintf and __sprintf_chk so sprintf_chk_ls_nlink is at most 1.05x glibc in instructions and 1.0x in time. Output bytes unchanged (compare against the current implementation over widths 0-40, left/right justification, precision cuts and multibyte-safe strings). Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both). Plants for new or changed code; proof by `cargo xtask bench --compare --counters` before/after on the rows above, no row of the same bench slower.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/. Plants run through `cargo xtask plants` only.

JOBS: gate, plants (`cargo xtask plants`), counts (before and after), percall
