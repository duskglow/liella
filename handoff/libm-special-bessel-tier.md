# LIBM-SPECIAL-BESSEL-TIER handoff (run 4, LIBM-SPECIAL-BESSEL-TIER-4: merge onto the gamma fast tier)

State: branch `task/libm-special-bessel-tier-4`, cut from LIBM-SPECIAL-GAMMA-TIER-3's head 24a78d42 (gamma tier plus
the coordinator's merge of origin/next e39b9799), plus a merge of `task/libm-special-bessel-tier-3` (d36d6fad):
merge 9ac0cff6, kernel index f5827152, compile fix + plants file 738dc681, then this handoff. Role mei:~/w/s2
(Skylake-SP); sync with `--base origin/next`. Accuracy contract unchanged (correct rounding, ruling #2760 A); no
differences row, no compat option, no new D-row.

## Run 4: what was done
* CI could not merge run 3 (conflicts in fast64.rs, special.rs, KERNELS.md, with next's float Bessel path
  LIBM-F32-BESSEL and the stacked gamma tier). Resolutions (merge 9ac0cff6):
  - `special.rs`: `j0/j1/y0/y1` try the float fast path (`PREC == 24`, `fast32::eval`), then the double fast tier
    (`PREC == 53`, `fast_bessel`), then `*_accurate`. The `*_accurate` helpers stay generic and `pub`: the same
    function serves fast32's unit tests (f32) and `tests/fast_bessel.rs` (f64).
  - `fast64.rs`: gamma already has `exp_dd` and `log_dd` (same bodies as this branch's): one copy kept.
  - mathtools: gamma's `special_tables::choose(slots, target)` made `pub(crate)`; `bessel_tables` calls it (was
    `choose_for`). Generator emits `gamma.rs` then `bessel.rs`. `cargo xtask tables`: every family file unchanged
    (bessel.rs and gamma.rs md5-identical to the committed copies).
  - `KERNELS.md`: regenerated with `cargo xtask kernel-index`, never hand-merged.
* Compile fix 738dc681: the gamma head 24a78d42 had `log_dd` inside next's `kernels!` macro with a plain
  `#[inline(always)]`; the FMA instantiation then calls the `#[target_feature(enable = "fma")]` `log_kernel` (E0133).
  Giving it the macro's attributes leaves an unused fused copy (dead_code). No FMA path calls `log_dd`, so it moved
  to module level after `reduce_pow` (where run 3 had it), body unchanged.

## Results (all on 738dc681, mei s2; later commits change handoff/ only)
* `libm-special-bessel-tier-4-lint3` rc 0: workspace clippy (all targets, all features, -D warnings),
  `cargo xtask kernel-index` (up to date), `cargo xtask static`. Superseded: `-regen` (rc 128, from my trailing
  `git status` in a non-git role dir; the generators themselves ran), `-lint` (E0133 above), `-lint2` (dead_code).
* `libm-special-bessel-tier-4-plants` rc 0 (handoff/libm-special-bessel-tier-4-plants.txt, dir
  /mnt/zram/claude/LIBM-SPECIAL-BESSEL-TIER-4/plants-1): 14 plants, 13 CAUGHT, 1 EQUIVALENT (near-zero, reason in the
  file), 0 SURVIVED, 0 WARN-RC-ONLY / WARN-NO-MSG. New: `entry-j0-tier`, `entry-y1-consult`, `accurate-generic` on
  the merged dispatch.
* `libm-special-bessel-tier-4-gate` (wide: `crates/math/src/lib.rs` is a crate root): every suite ok through
  `test -p liella-math` except ONE test, `liella-boundary::search_nomem hcreate_and_hsearch_fail_with_the_table_intact`
  (exports interposed): child exit 14 = "the limit never bit" (40 000 hsearch ENTERs all fit under the RLIMIT_AS
  headroom). Not this change (libm only; the same test passed in the non-interposed run). Evidence it is
  pre-existing on this host: mei's CI gate logs hold 12 such failures (exit 14 x8, exit 10 x4, both "limit never
  bit") on 4 other commits (ci-gate-18077d4d1, -635a721bb, -cbd522f24, -d999f7104); none on keke, shiki, kanon.
* `libm-special-bessel-tier-4-counts` (`cargo xtask bench --compare --counters cbench_libm`, starts after the gate):
  see the report file / hand-back for the j0/j1/y0/y1 rows; expected equal to run 3 (j0 1.20x, j1 1.22x, y0 1.14x,
  y1 1.15x on Zen 1) since the double path's code is unchanged; this run is on Skylake-SP.

## Open / next
* RULING NEEDED: the gate's only failure is the pre-existing mei `search_nomem` hsearch test above; recommend
  accepting this gate for the story and opening a separate story to make that test's headroom robust (the
  ballast/RLIMIT_AS sizing does not always bite on this host).
* Cost (from run 2/3): Taylor rows take 8 double-double steps; an FMA build of the kernel (`two_prod_fma`, D-10.25
  dispatch) is the next cut, then hot boundary wrappers in `hot/bessel.rs`.
* No test reaches `special::j0::<f32>`'s entry dispatch (fast32's tests call `fast32::eval` directly); a plant there
  would need a conformance program.
* `xtask affected` escalates the generator `crates/mathtools/src/tables.rs` and crate roots to wide/FULL.

## Not to redo
* The ODE recurrence is stable at 640 bits. The merge: tables are per family; regenerate only on a generator change.
