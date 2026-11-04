# PERF-CTYPE-LOC (N3-7)
Branch task/perf-ctype-loc, code head dc91b75d (before the report commit). Host kanon role kn1.
Cause (measured, not a guess): `__ctype_b_loc` compiled to push/pop of the alignment slot around the cold `ctype_fill` call,
so the filled path ran 8 instructions (push, mov %fs, test, je, mov %fs:0, add, pop, ret). Baseline (job perf-ctype-loc-counts-before2, cbench_hot default, kanon): ctype_b_loc 14.0 vs glibc 10.0 instr (1.40), cyc 0.83.
Change: ctype_slot returns ctype_fill(slot)'s result (tail call), so the filled path has no frame (6 instructions).
Proof: clippy --workspace -D warnings rc 0, cargo xtask static rc 0 (foreground, on this head). Plants job perf-ctype-loc-plants3 rc 0: 3 CAUGHT
(tolower/toupper/b offset swap, tests sym___ctype_*). NOT COVERED: the cold fill path (no test makes a thread the runtime does not list; plants
that break ctype_fill's result or skip refresh_words SURVIVED in job perf-ctype-loc-plants2). Counters after: job perf-ctype-loc-counts (hot default, hot -AVX2, locale default).
Gate: job perf-ctype-loc-gate (waits for the counts job, then direct-tier gate).
NOT DONE: before counts at -AVX2 and cbench_locale (the first baseline job was killed at 50 min; slow bench, ~35 min per invocation); wl_count of cut-tr-uniq; timing on booked hosts.
Next: read the three jobs, fill the register row with before/after (before locale/isalpha_c is the register's 22/16).
