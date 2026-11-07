# XTASK-BENCH-TUNABLES-AND-MATHTOOLS (handoff)

State: branch task/xtask-bench-tunables-and-mathtools, code head 0b8f53ce9 (+ this handoff commit). Decision in force: D-X.39 (plan/07-decisions/x-implementation-decisions.md).

## What was done
1. bench: the caller's `GLIBC_TUNABLES` was dropped silently (env_clear in `stress_command_in`, BENCH-FIXED-ENV), so
   `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA cargo xtask bench` measured the default level. Now `resolve_tunables`: `--tunables`,
   else a non-empty caller value; a conflict or a non-UTF-8 value is an error. Applied in `compare` and the plain run.
   Other routes: the netdb/nscache private-/dev/shm route goes through `sudo` (which resets the environment): the value is now
   an argument of its `sh -c` script. Rust example benches get it via `with_tunables`. The `--base` nested run inherits the env
   and forwards the args, so it resolves the same.
2. affected: `mathtools_feeds`: remez.rs, bin/remez.rs -> `remez --check <every fit>` (one command); bessel/gamma/special_tables.rs,
   bin/tables.rs -> `tables --check` + `kernel-index --check`; lib.rs and mp/*.rs -> both; cmp.rs, cplx.rs, bin/mathcmp.rs -> none
   (the comparison tool feeds no generated file); tables.rs unchanged (XTASK-AFFECTED-MATH-2). Each keeps its own mapping.
Tests: bench::tests::caller_glibc_tunables_is_honoured_or_refused, private_shm_route_carries_the_tunables_value (prints SKIP
where sudo -n or setpriv is absent), affected::tests::mathtools_generator_inputs_select_their_checks. Docs: docs/benchmarks.md, usage line.

## Results (keke, build role)
- xbt-check2: cargo xtask clippy, cargo xtask static, 43 selected xtask unit tests: all green.
- xbt-gate2 (affected --run --base origin/next), xbt-plants3 (7 plants, handoff/XTASK-BENCH-TUNABLES-AND-MATHTOOLS-plants.txt),
  xbt-counts (cbench_libm/exp: default, caller GLIBC_TUNABLES=-FMA, --tunables -FMA, and a conflict that must be refused):
  RUNNING at hand-back; re-attach: `~/bin/jobwait xbt-gate2 xbt-plants3 xbt-counts` on the role host. NOT VERIFIED until they finish.
- SUPERSEDED: xbt-check (compile error: `gen` reserved in edition 2024), xbt-gate, xbt-plants (killed/failed with that error), xbt-plants2 (plants file refused: a ` | ` inside a lit: pattern).

## Open / informational
- The sudo route also passes CBENCH_ROWS / CBENCH_COUNTERS through the environment (`${CBENCH_ROWS:+...}`), which sudo's env
  reset may drop as well; not changed here (outside the story). Next: same argument treatment if netdb counts lack ctr lines.
- Proof row choice: exp (Liella's FMA build, D-10.50) instead of strstr_hit_64 (not a literal row name in bench/c).

## Next step
Read xbt-counts: Liella and glibc exp counts must differ between default and -FMA, env and option must agree, and the
conflict line `conflict-refused-ok` must print. Then gate/plants green -> ready.
