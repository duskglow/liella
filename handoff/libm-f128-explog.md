# handoff: libm-f128-explog (run 1: design; -2: code, unit-tested; -3: merged next, plants, sweep, percall, record)

**State.** Branch `task/libm-f128-explog-3` (= `task/libm-f128-explog-2` + merge of origin/next a5636ce6b + the
commits below). The binary128 `expf128`/`logf128` three-limb integer tier is done:
`crates/math/src/wide_int/quad/explog.rs` (exp X1-X6, log L1-L4; the module doc holds the method, citations and
error budget), wired in `wide.rs::tiers()` after the x87 tier; quad's `round` delegates to `round_within(.., err)`;
generator `crates/mathtools/src/ld_explog_tables.rs` adds the WQ_* third limbs (2.5 KiB); long sweep registered in
`xtask/src/longsweeps.rs` (that one line makes `cargo xtask affected` escalate the gate to FULL: xtask/src/longsweeps.rs
is not in XTASK_TOOLS).

**Commits in -3.** Merge of origin/next (clean); plants file (`handoff/LIBM-F128-EXPLOG-3-plants.txt`, now with the
tool's results); `validation/percall/libm_f128_explog.c` (expf128 typical/wide, logf128 typical/near 1); the pending
decision `records/pending/LIBM-F128-EXPLOG-3-1.toml` (D-NEW, series D-10: method Tang 1989/1990, Wong-Goto 1994,
Brent-Zimmermann 2010; bounds; tables; measurements); a fix of bare `|` in that record (records check).

**Results (kanon, Broadwell-EP; jobs in kanon:~/logs/).**
- `libm-f128-explog-3-lint` (inherited head after merge) and `-lint3` (final code head 19dbc5ad1): clippy + static rc 0.
  `-lint2` rc 1 was the record's bare `|`, fixed in 19dbc5ad1.
- `libm-f128-explog-3-plants`: 8 plants, 7 CAUGHT, 1 EQUIVALENT (q-err-bound: the rounding bound moved by 2^-187;
  the bound has 2^1.7 slack over the proved error, so no operand can tell; the bound rests on the module-doc proof).
- `libm-f128-explog-3-longsweep` (release, 10^7 operands, twelve bands incl. overflow/underflow/subnormal and near 1,
  four modes vs the 384-bit route): 33,333,160 calls 0 undecided, 0 refused, worst 2^-141.96; near 1: 6,666,632 calls,
  16 undecided (go to the 384-bit route), worst 2^-146.53. rc 0.
- `libm-f128-explog-3-percall` (instructions:u per call, release; glibc / next a5636ce6b / new):
  expf128 6682 / 33418 / 767; expf128_wide 6704 / 33533 / 746; logf128 7794 / 70524 / 898;
  logf128_near1 7481 / 48493 / 884. All 0.11-0.12x glibc; self-checks ok.
- `libm-f128-explog-3-gate2`: `cargo xtask affected --run --base origin/next` on 19dbc5ad1 (escalated to FULL by
  longsweeps.rs; see the hand-back for its state). `libm-f128-explog-3-gate` (first try) was stopped by me after it
  escalated, before the percall/record commits; superseded.

**Not done / open.** FMA-off (`glibc.cpu.hwcaps=-FMA`) counts not run: the tier is integer-only (no FP ops), so
Liella's count does not depend on the path; only the reference column could move. Timing (Ivy Bridge, AVX2 host) not
measured: left to the periodic timing sweep (instruction mix changes from 384-bit software arithmetic to 64-bit
integer multiplies, 40-80x fewer instructions). The pending record needs an id from the coordinator.

**Decisions in force.** D-10.52, D-10.73 (x87 tier, tables shared), the pending record above.

**Do not redo.** Design and proof (module doc), tables, unit tests, plants, long sweep, percall.

**Next step.** If gate2 is green: land. Then the next story: log2/log10/log1p/expm1/exp2/exp10 for binary128 on the
same kernels (`exp_kernel`, `ln_kernel` in explog.rs), and consider adding `xtask/src/longsweeps.rs` to the affected
tool's non-escalating table (it only lists test names) so a new sweep does not force a FULL gate.
