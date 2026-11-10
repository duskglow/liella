# handoff: libm-ld-explog-int (LIBM-LD-EXPLOG-INT, -2)

**State.** Branch `task/libm-ld-explog-int-2` (stacked on `task/libm-ld-explog-int`, from origin/next 21ef00b34).
Kernels, proof, wiring and tests built and verified on keke:~/w/r5 (run -2). Code head `bc427462a`; later commits are
handoff/report only. Decision record D-10.73 written (`records/decisions/D-10/D-10.73.toml`).

**Done in -2.**
* Built the run-1 WIP (it had never compiled). Fixed: the test band table (3-tuples); two unit slips in the kernel
  (`a·h1` is in units 2^-135, so `exp_table` shifts by 7; the log series `w·h1` is in units 2^-139, so the sums shift
  by 12 / 26); the constant pins' helper (zero and sub-unit values). The test now refuses-and-skips results
  outside the normal range, skips the operands the entry points settle first (`expm1` x < -50.8, `log 1`), and keeps
  structural hairlines (1 ± d·2^-63 with d rich in trailing zeros) out of the near-1 random band (they stay in `hard()`).
* Results, `tier_agrees_with_the_wide_route` (3 200 random per function, four modes, plus hard operands): 0 undecided
  in the random bands; worst error exp/exp2/exp10 2^-112.2, expm1 2^-104.1, log 2^-104.5, log2 2^-104.2, log10
  2^-105.5, log1p 2^-106.2 (proof: 2^-110.4 / 2^-101.8 / 2^-103.2).
* Long run, 10^7 operands per function in four modes (job libm-ld-explog-int-2-acc, before the band fix): exp, exp2,
  exp10, expm1: 0 undecided in ~1.5·10^8 calls each, worst 2^-112.0 / 2^-103.6; log: 0.08% undecided. Rerun after
  making the near-1 band's distance odd (-acc2): log still 0.064% undecided (25 504 of 4·10^7), so the "structural
  hairline" explanation covers at most part of it. **OPEN, first step of -3**: print the undecided log operands
  (by band) from the long test, find the path that loses precision or the operand class, fix; the test's limit is
  1e-5. log2/log10/log1p long runs not reached yet (the test stops at the first failing function). Undecided calls
  are correct (they go on to the double-double tier), only slower.
* static (lint3) failed on a bare `|` in the D-10.73 body; fixed after the jobs started (code unchanged).
* Plants `handoff/LIBM-LD-EXPLOG-INT-plants.txt`: 5 of 5 CAUGHT with msg (polynomial shift, reduction low part,
  `near` always true, expm1 switch, log series units).
* clippy + static clean (lint2).

**Jobs on the final code head (keke).** libm-ld-explog-int-2-lint3 (clippy + static), -acc2 (10^7 long run),
-plants2, -gate (`affected --run --base origin/next`). Re-attach: `ssh keke '~/bin/jobwait <label>...'`.

**Not done (LIBM-LD-EXPLOG-INT-3).**
1. #4209: move the table generator into `cargo xtask tables`: a new family in `crates/mathtools/src/tables.rs`
   writing `crates/math/src/coeffs/wide_int_el.rs` (`@generated`), with the constants and tables (C_E .. G3, EXP2T,
   C1, L1, C2, L2) removed from `wide_int_el.rs`. The run-1 generator is `~/w/.scratch/LIBM-LD-EXPLOG-INT/gen.py`
   (Python decimal, 90 digits; also checks |u1| <= 2^-7 per interval): port its formulas, not its code. Until then
   every value is independently re-derived in-repo by `constants_are_the_rounded_values` (384-bit `mp`).
2. Percall counts: add `validation/percall/libm_ld_explog.c` on the pattern of `libm_ld_trig.c`; counts for the 8 rows.
3. Booked timing, Ivy Bridge (nico or kotori, the quieter) and N100 (nozomi); target <= 1.5x glibc cycles per row.
4. Saturation entry and codemap if static asks; then fill the timing line of D-10.73 (a new record that `amends`).

**Do not redo.** The design and proof (module doc of `wide_int_el.rs`); the unit fixes above; the plants.
**Read.** D-10.52, D-10.56, D-10.73; `crates/math/src/wide_int_el.rs` (module doc); `wide.rs` `tiers()`.
