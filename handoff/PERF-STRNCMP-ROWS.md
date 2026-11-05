# PERF-STRNCMP-ROWS handoff
Branch task/perf-strncmp-rows, code head 4f01a90f (the report commit follows it). Ran on keke (Zen 1 EPYC 7551, AVX2).
Change: csys/src/machine/scan.S, liella_machine_strncmp AVX2 head: zero operand from rodata (.Lstrcmp_zero32, no vpxor);
decided-path re-read keeps the index in rax (no movq); no n<=32 exit before the second block (same pages, hit>=n gives 0); second-block hit tail duplicated (no jmp).
Counts (instructions:u/call Liella vs glibc, cbench_hot, --runs 3): eq16 36.1->34.0 (glibc 34.0), diff8 36.1->34.0 (34.0), names64 47.1->42.1 (43.0). Other rows: only rounding (.1) changes.
Plants (5, each timeout 600, all CAUGHT): off-by-one bound, offset dropped, sign flip, index lost, wrong zero operand.
Jobs: PERF-STRNCMP-ROWS-counts-before/-counts-after/-plants/-gate/-static. Time ratio was not the criterion (instructions decide; time not re-booked).
Open: n<=32 strings that are equal for 32+ bytes now read one more 32-byte block (inside the page); no measured row has that shape.
Not done: no percall spec added; saturation-register row not updated (rows now at parity).

## PERF-STRNCMP-ROWS-2 (continuation)
Cause of the first gate's red (job perf-strncmp-rows-gate, empty log): the inline decided path in the head moved the byte to ecx and left the
index in eax, so an equal-NUL hit returned the index (liella-boundary machine_scanners page-edge test aborted with "string scanner postcondition
failed"; enomem_get_current_dir_name in the interposed run was a knock-on of the same wrong return). Fix: both inline paths take .Lsnc_zero on NUL
(same instruction count on the taken-less common path). Reran under new labels: perf-strncmp-rows-2-gate3 (clippy, static, affected --base origin/next),
plants: perf-strncmp-rows-2-plants2 (6 plants: 5 CAUGHT, hit-index-lost EQUIVALENT: that index is read only when a writer changes the bytes between scan and re-read; new plant nul-returns-index CAUGHT).
Counts not rerun: the fix changes no executed instruction count on measured rows (a not-taken jump target only); earlier counts 34.0/34.0/42.1 stand.
SUPERSEDED: perf-strncmp-rows-2-gate2 (killed, stale head), perf-strncmp-rows-2-plants (first plant set, hit-index-lost marked SURVIVED by the script).
