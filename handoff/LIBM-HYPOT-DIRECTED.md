# LIBM-HYPOT-DIRECTED: hardware math operations through a trait; hypot first (#4967)

State: branch `task/libm-hypot-directed-2` (continues `task/libm-hypot-directed` 490dcb9d7; origin/next ca154363f merged
in), HEAD in handoff/reports/LIBM-HYPOT-DIRECTED-2.json. Role keke:~/w/n1 (Zen 1). Code head 4c5326b09 (the merge);
later commits are handoff/ only.

## Decisions in force

* D-X.75 (the `HwOps` trait pattern: `liella_math` generic over hardware operations, `SoftOps` default,
  `liella_boundary::hot::Hw` on `sqrtsd`), D-10.112 (amends D-10.104: the directed/fallback hypot kernel takes its
  first root from `hot::Hw`). Owner ruling 2026-11-12 on the trait design. Rulings #4989 (ids), #4990 (follow-up
  LIBM-HYPOT-DIRECTED-FAST, held until this lands).

## What was done

* -1 (LIBM-HYPOT-DIRECTED): `liella_math::hwop::HwOps` + `SoftOps`; `invtrig::hypot_rounded::<O>` / `hypot_tiny::<O>`;
  `pow::hypot_with::<F, O>`; the `double` `hypot` entry's fallback passes `hot::Hw`. Tests: hard pairs in
  `crates/math/tests/support/hypot_pairs.rs`, run on `SoftOps` (math) and on `Hw` plus the entry under all four
  `fesetround` directions (`s10_math_symbols::hypot_on_the_hardware_root_is_the_exact_path_in_every_direction`).
* -2 (this run): CI was red on hot_text_products (fat LTO inlined `Vdso::lookup_many::<5>`, the `--allow-drop` regen
  then dropped `global_locale::GLOBAL` from startup-data.keys). LAYOUT-HOTKEY-INLINE-STABLE has landed (it pins
  `lookup_many` `#[inline(never)]`), so: merged origin/next, and took every generated list under crates/libc-so
  (hot-text.list, startup-text.keys, probe-text.keys, startup-data.keys) back to next's bytes. No regen was needed:
  both profiles build and hot_text_products passes with next's lists, so the branch changes no list and drops no
  key (startup-data.keys keeps `global_locale::GLOBAL` in both profiles). Only merge conflict besides the lists:
  s10_math_symbols.rs (next removed the unused `skip_note` module; this branch's `hypot_pairs` kept).

## RESULTS (keke, Zen 1; labels are ~/logs/<label>.log)

* `libm-hypot-directed-2-lint0`: clippy + static on the inherited head 490dcb9d7: rc 0.
* `libm-hypot-directed-2-hot` (code head): `cargo xtask so` + hot_text_products, fast then release: 24 passed /
  0 failed in each profile; `init_data_clusters_hold_at_every_text_phase` and
  `init_short_programs_stay_within_their_page_fault_bounds` ok in both (every text/data phase, the tests' own loops).
* `libm-hypot-directed-2-plants` (tool, file handoff/LIBM-HYPOT-DIRECTED-2-plants.txt): entry-nearest, hw-root-off,
  kernel-up-nearest: 3 CAUGHT, 0 WARN.
* `libm-hypot-directed-2-percall3` (validation/percall/hypot.c, instructions/call, base next ca154363f / head / glibc):
  hypot_typ 105.0 / 105.0 / 65.9; hypot_near 105.0 / 105.0 / 68.8; hypot_bits 63.1 / 63.1 / 41.5 (nearest rows
  unchanged); hypot_near_up 329.6 -> 296.6, glibc 68.8: 4.31x (target "small factor" not reached; follow-up
  LIBM-HYPOT-DIRECTED-FAST).
* `libm-hypot-directed-2-gate` (affected --run --base origin/next, 240 of 266 targets, 132 conformance programs):
  rc 0, 11 suites ok, 11687 passed / 0 failed, conformance 132 / 0, peak 12G, oom_kill 0.
* `libm-hypot-directed-2-sweeps` (on the gate's products): s10_math_symbols hypot* 4 passed (incl.
  hypot_on_the_hardware_root_is_the_exact_path_in_every_direction), invtrig hypot* 2 passed (the kernel in every
  direction; the fast tier on random operands), elem_products 1 passed (the C differential of hypot against the
  host library in all four rounding directions): rc 0.
* `libm-hypot-directed-2-lint1`: clippy --all-features -D warnings, cargo xtask clippy, cargo xtask static on the
  code head (see the report json for its rc).
* Superseded: `libm-hypot-directed-2-percall` (base given as a short sha: refused), `-percall2` (base not primed on
  the host; primed with validation/wl_prime_base.sh, then -percall3).

## Open / next

* Next: LIBM-HYPOT-DIRECTED-FAST (the directed envelope `elem_switched` and the non-FMA kernel hold the surplus,
  not the root). Optional: move cabs/cpow (`pow::hypot` on `SoftOps`) to the trait; add `fma` to `HwOps` when a
  kernel needs it.
* Do not redo: the trait design, the D-10.104 kernel, a hot-text regen (none needed on next ca154363f).
* Plan rows: D-10.104, D-10.112, D-X.75, D-10.13, D-10.22, S1/S2 (plan/02-design-rules.md).
