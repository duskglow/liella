Id: COLDSTART-SECOND-LATENCY
Kind: code
Priority: 40
Size: S
Host: a build host for counts and profiles; the N100 timing host (booked) for time
Model: opus
Effort: high
BUDGET: 40 tool calls / 80k
Stack on: next (730f416b or later)

# second_setlocale and second_mktime: time without instructions

## Rows
cbench_opt/second_setlocale_en_US: 1.43x (N100), 1.35x spread 0.46 (Ivy Bridge). cbench_opt/second_mktime_new_york: 1.28x spread 0.085 (N100), 1.11x spread 0.46 (Ivy Bridge).

## Evidence
Probe: second "S" Liella +177 user instructions vs glibc +174; second "M" Liella +1064 vs glibc +1266; no page faults on either side (Broadwell-EP). The instruction count does not explain the time ratio.

## Hypothesis
A latency cost (cache or TLB misses on state spread over more lines or pages, or a syscall/vDSO read) or noise of a single timed call. Step 1: the N100 re-time (COLDSTART-PLAN-1106 staged bench, label CSP1106-n100) decides; if the row is at or below 1.05x, close this story as fixed / noise. Otherwise count cycles, dTLB-load-misses and L1d misses per call (aggregate, both sides) over many fresh processes and name the lines/pages touched on the Liella side.

## Tests and plants
Only if code changes: the setlocale and mktime conformance tests; plants on the changed code.

## Rules
No eager work at start-up or on the hot path: lazy work stays on the first-use path, and that path is still measured (failure-path-cost rule). Re-time on the N100 timing host with cargo xtask bench --stage / --staged; Liella profiles by symbol on a build host; glibc aggregate counters and timing only.

**JOBS:** gate (direct tier, --base origin/next), plants (cargo xtask plants), counts (the probe deltas above, before/after/glibc), timing (N100 staged bench of the rows) as <host>:<label>.

First line of the hand-back: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:.
