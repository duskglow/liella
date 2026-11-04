# XTASK-BENCH-NAMES-2
State: branch task/xtask-bench-names-2. Ported XTASK-BENCH-NAMES (one commit, authorship Liella Maintainers): `bench --compare` checks every name against bench/c/*.c before the product build (xtask/src/bench.rs check_bench_names); error lists valid benches and says --compare has no row filter. Usage text in xtask/src/main.rs updated.
Conflict resolution: bench.rs conflicted only in the tests module. Next's TEST-BENCH-TUNABLES-LEAK comment is kept (it replaces the older TEST-ENV-TUNABLES comment, next already does that another way); the new test compare_rejects_unknown_bench_name_listing_valid_ones sits before it.
Decision: row filter NOT added to --compare (rows are chosen inside each C program); documented in the error and usage text.
Plant: check_bench_names made a no-op: CAUGHT. Call removal in compare(): EQUIVALENT for the unit test; the real `xtask bench --compare exp` is rejected before building (checked in the plants job).
Next: none.
