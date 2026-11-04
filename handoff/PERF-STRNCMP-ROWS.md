# PERF-STRNCMP-ROWS handoff
Branch task/perf-strncmp-rows, code head 4f01a90f (the report commit follows it). Ran on keke (Zen 1 EPYC 7551, AVX2).
Change: csys/src/machine/scan.S, liella_machine_strncmp AVX2 head: zero operand from rodata (.Lstrcmp_zero32, no vpxor);
decided-path re-read keeps the index in rax (no movq); no n<=32 exit before the second block (same pages, hit>=n gives 0); second-block hit tail duplicated (no jmp).
Counts (instructions:u/call Liella vs glibc, cbench_hot, --runs 3): eq16 36.1->34.0 (glibc 34.0), diff8 36.1->34.0 (34.0), names64 47.1->42.1 (43.0). Other rows: only rounding (.1) changes.
Plants (5, each timeout 600, all CAUGHT): off-by-one bound, offset dropped, sign flip, index lost, wrong zero operand.
Jobs: PERF-STRNCMP-ROWS-counts-before/-counts-after/-plants/-gate/-static. Time ratio was not the criterion (instructions decide; time not re-booked).
Open: n<=32 strings that are equal for 32+ bytes now read one more 32-byte block (inside the page); no measured row has that shape.
Not done: no percall spec added; saturation-register row not updated (rows now at parity).
