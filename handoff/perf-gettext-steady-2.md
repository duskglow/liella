# PERF-GETTEXT-STEADY-2 (branch task/perf-gettext-steady-2)
State: measured and documented, NO library change (docs/saturation/14 "Row 6 run 2"). Counts job perf-gettext-steady-2-before,
profile job perf-gettext-steady-2-prof (keke, both rc 0). Steady 712.8/452.6 = 1.58 i; found 602.8/452.6 = 1.33.
Not done: the aligned fused LANGUAGE compare (unsafe in liella-boundary, ~40 of 110 instructions), clock-free recheck.
Next step: decide the unsafe category for an aligned NUL-safe read of the environment value, then implement with a plant that
skips the in-place-rewrite check (must be CAUGHT) and a short-value-at-page-end test.
Budget note: ~30 of 45 calls spent on reading, two jobs and a failed first ssh (host names do not resolve; use 10.0.100.53).
