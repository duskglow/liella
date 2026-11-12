# LIBM-HYPOT-DIRECTED: hardware math operations through a trait; hypot first (#4967)

State: branch `task/libm-hypot-directed` (from origin/next 6175794f1), HEAD in the report json. Role shiki:~/w/sh2.

## What was done

* `liella_math::hwop::HwOps` (new, safe): operations a kernel may be generic over; contract = the weaker of the
  implementations (`sqrt`: within 2^-50 relative for a normal positive operand). `SoftOps` = safe default (the former
  `invtrig::sqrt_soft`, moved). `invtrig::hypot_rounded::<O>` and `hypot_tiny::<O>` take the first root from `O`;
  `pow::hypot_with::<F, O>`; `pow::hypot` keeps `SoftOps` (cabs, cpow, other callers unchanged).
* `liella_boundary::hot::Hw` implements it on the existing `hot::sqrtsd` (hot-kernel category, no new unsafe site).
  The `double` `hypot` entry's fallback (directed modes through the switch, fast-tier refusals, DAZ/FTZ/traps) passes
  `Hw`. Both fallbacks run in the default MXCSR, so `sqrtsd` is correctly rounded to nearest (2^-53): inside the
  2^-50 the D-10.104 proof assumes (`sqrt_dd` stays < 2^-99.5 under Ziv's 2^-98); the integer root is still fixed
  by exact squares.
* Tests: the kernel's hard pairs moved to `crates/math/tests/support/hypot_pairs.rs`; the math test runs them on
  `SoftOps`; new `s10_math_symbols::hypot_on_the_hardware_root_is_the_exact_path_in_every_direction` runs them on
  `Hw` and through the entry under all four `fesetround` directions (bits, errno, flags) against `hypot_exact`.
* Records (pending, ids to assign): records/pending/LIBM-HYPOT-DIRECTED-1.toml (D-X: the trait pattern),
  -2.toml (D-10, amends D-10.104). Design-rules note after the S table in plan/02-design-rules.md.
* Lane: hot-lists. The new monomorphisation shifted the boundary's codegen: `Vdso::lookup_many::<5>` is now
  inlined into its caller in the fast profile, so its hot-text.list key matched no section and `cargo xtask so`
  failed (base 6175794f1 builds). Regenerated with `tools/hottext/regen.sh . --build --allow-drop` (fast) and
  `LIELLA_FULL_PROFILE=1 ... --build` (release); see RESULTS for the files it changed.

## RESULTS

All jobs on shiki, code head b3e4ee5ad (later commits: handoff/ only).

* Counts `libm-hypot-directed-percall2` (validation/percall/hypot.c, Haswell-EP build host, instructions/call,
  base 6175794f1 / new / glibc): hypot_typ 106.0 / 106.0 / 65.9; hypot_near 106.0 / 106.0 / 68.8; hypot_bits
  64.1 / 64.1 / 41.5 (nearest rows unchanged); **hypot_near_up (FE_UPWARD) 355.6 -> 318.6, glibc 68.8: 4.63x.
  Target "small factor" NOT reached**: the root was only ~37 of the 355 instructions. The rest is the directed
  entry itself: `elem_switched` (MXCSR switch, operand/result handling, flag settlement: 161 lines, 6 calls) around
  an out-of-line `hypot_rounded::<Hw>` (488 instructions of code, the plain non-FMA build: Dekker products).
* Disassembly (percall products of b3e4ee5ad, libm.a, `objdump -dr`): `hypot` calls `elem_fast_cold` and
  `elem_switched` (cold), each calls `liella_math::invtrig::hypot_rounded::<liella_boundary::hot::Hw>`; in it
  `sqrtsd` is inline at +0x17c (main root) and +0x294 (both-subnormal integer root); no call to a root function
  remains (its 4 GOT calls are helpers off the main path; not identified further).
* Plants `libm-hypot-directed-plants2` (tool, ~/work/LIBM-HYPOT-DIRECTED/plants.txt): entry-nearest (fallback given
  Rounding::Nearest), hw-root-off (Hw root x1.0001), kernel-up-nearest (directed branch drops the upward step):
  3 CAUGHT, 0 WARN.
* Gate `libm-hypot-directed-gate2` (affected --run --base origin/next): rc 0, 32 suites ok, 11646 passed, 0 failed
  (init_data_clusters_hold_at_every_text_phase passes after the regen).
* Checks job `libm-hypot-directed-t3` (clippy, static incl. records check: 2 pending, the two tests): rc 0.
* Superseded: t1/t2 (lint red, fixed), gate/plants/percall (first code head: `cargo xtask so` refused the stale
  hot-text key), hotregen (refused to drop without --allow-drop; hotregen2 did it, rc 0).
* Regen side effect (INFORMATIONAL): startup-data.keys lost `global_locale::GLOBAL` (rw, fast) and NSCACHE gained
  the fast profile; probe-text.keys lost the lookup_many key and a `TscGate::fresh` key. Taken whole, as the tool wrote it.

## Open / next

* Next step (optional): move cabs/cpow (`pow::hypot` on `SoftOps`) and other kernels with a software root to
  `hypot_with::<_, Hw>` / the trait; add `fma` to `HwOps` when a kernel needs it.
* Do not redo: the trait design (owner ruling 2026-11-12), the D-10.104 kernel.
* Plan rows: D-10.104, D-10.13, D-10.22, S1/S2 (plan/02-design-rules.md).
