# TIMING-COV-GETLINE-LONG: getline on 10,000-byte lines

Id: TIMING-COV-GETLINE-LONG
Kind: code
Priority: 50
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 70 calls / 140k

## The change
Cut getline / getdelim's per-call layers for a line longer than the stream buffer (crates/legacy/src/stdio_compat.rs
getdelim, crates/boundary getdelim_general, liella::stream State::read_until_with). No open story to stack on.
PERF-GETLINE-LONG (red, WIP, 3592 -> 3108 instr per call in its bench shape) and PERF-GETLINE-LONG-2 (red: all 6 plants
survived, its test did not reach the partial path) were never landed: ask the coordinator whether to continue from
task/perf-getline-long-2, and if so make its test reach the partial-buffer path first.

## Rows (TIMING-SWEEP-1105)
getline_long: Ivy Bridge instr 1.201 / time 1.080; N100 1.168 / 1.009.

## Evidence (keke, next 730f416b, non-AVX2 path, job timing-coverage-1106-percall)
gl_long (validation/percall/strstr_getline.c): Liella 5170.6 / glibc 4251.8 instructions per call (1.22). One traced
getdelim call (call 21, 505 instructions; Liella only): stdio_compat::getdelim 145, State::read_until_with 96,
getdelim_general 77, stdio_compat::load 56, the exported entry 43, Live::read 40, store_read 30, syscall 10,
position 8: about 360 of 505 are layer crossings (load / store of the stream state, three wrappers) rather than scanning.
NOT VERIFIED: call 21 may be a short refill call; trace a full-line call (larger SKIP) in step 1.
Workloads: getdelim 2.0M calls in 2 of 30 workloads.

## Hypothesis
Each buffer refill pays the load / store of the stream state and three wrapper layers again; on a 10 KB line that is
paid per refill. Keep the state loaded across refills inside one getdelim call.

## Steps
1. Trace a whole gl_long call (wl_percall --trace getdelim:<skip>:60000) and count refills per line.
2. Hoist load / store_read out of the refill loop.
3. Tests: getline / getdelim with lines of 1, buffer-1, buffer, buffer+1 and 3x buffer bytes, EOF without newline,
   read error mid-line (errno, partial result), sticky EOF; the partial path must be reached (prove it with a plant).
4. Plants: drop the last refill's bytes, lose the stream position after a refill, skip the store on error.

## JOBS
gate, plants, counts before / after (wl_percall --spec validation/percall/strstr_getline.c --cases gl_80,gl_long),
timing nico:timing-cov-getline-long-time (`ops timing-book nico <min> --story TIMING-COV-GETLINE-LONG`).

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
