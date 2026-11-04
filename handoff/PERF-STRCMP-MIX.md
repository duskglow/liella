# PERF-STRCMP-MIX handoff

State: branch task/perf-strcmp-mix. Change: AVX2 strcmp head blocks use the stop-locating `bsf` as the empty-mask test (csys/src/machine/scan.S), plus a percall spec validation/percall/strcmp.c. Register row: docs/saturation/15-...md section 15.3.

Result (keke, Zen 1 host, percall instr/call, glibc / base / new): strcmp_16_mix 50.5 / 61.2 / 60.3; strcmp_64_mix 65.4 / 78.3 / 77.4; strcmp_names 38.0 / 46.9 / 46.4. Only 0.5-0.9 instruction recovered; the 1.05x target is NOT reached. Bound: fixed entry cost (level test 2, page test 8, sbb/or/ret). Hypothesis (branch behaviour on strcmp_names) was not what the counts show: it is instruction count, not branches.
Not redone: OR/XOR page tests and a 32-byte first block (rejected in the head comment).
Ideas left: dispatch the level test away (patched entry), a one-register page test for both strings. Both need a ruling or a timing booking.
Not verified: time rows (no timing booking was made; the change only removes instructions). Bench --compare --counters rows not run; percall used instead.
