# handoff: libm-f128-explog (LIBM-F128-EXPLOG run 1: design; LIBM-F128-EXPLOG-2: code, unit-tested)

**State.** Branch `task/libm-f128-explog-2` (on `task/libm-f128-explog`, from origin/next d1b3a596c); head = the
WIP commit "Add the binary128 exp/log three-limb integer tier". Run -2 stopped at its 180k context checkpoint.
Done: `crates/math/src/wide_int/quad/explog.rs` (exp X1-X6, log L1-L4; the module doc holds the method, citations
and error budget), wired in `wide.rs::tiers()` (after the x87 tier); quad's `round` now delegates to
`round_within(.., err)` (same code, bound as parameter); generator `crates/mathtools/src/ld_explog_tables.rs` adds
`third()` and the WQ_* tables (regenerated with `cargo xtask tables`, job rc 0); long sweep registered in
`xtask/src/longsweeps.rs`. Job `libm-f128-explog-2-unit2` (shiki, final head): clippy + static + `cargo test -p
liella-math --lib wide_int::quad` green: sweep 6016 calls, 0 undecided, worst 2^-142.2; near 1: 1200 calls,
0 undecided, worst 2^-148.1.

**Fixed in -2 (not in the run-1 design).** `round_exp`: run 1 said "None if s >= 192"; that refuses every
result in [2^-16495, 2^-16494) (be = -112, the whole bottom band). Now s = 192 takes the normal test, s > 192
rounds as a tiny value (Exact sig 0, sticky), refusing only be = -113 with M's top word all ones.

**Decisions in force.** D-10.52, D-10.73 (x87 tier). No new record yet.

**Next steps, in order.** (1) Direct-tier gate on the head (`cargo xtask affected --run`, base origin/next).
(2) Plants through `cargo xtask plants` on explog.rs, msg regex `explog::tests.*(FAILED|panicked)`: Horner
shift (`mulhi64(a1, h) >> 12` -> 11), third-limb sign (`signed3` `lo < 0` -> `lo > 0`), `s` formula (`80 - be` ->
`81 - be`), ERR (`+ 16` -> `- 16`... check it is caught, else `equivalent:`), i2 rounding (`1 << 116` -> `1 << 115`),
(b)/(c) shifts (`sar(l2, 6)` -> 7, `sar(l1, 14)` -> 13), `p.shr(lzw + 14)` -> 13. (3) percall for expf128/logf128
against next and glibc (`validation/wl_percall.sh`; the f128 rows' file is not under validation/percall/ by
name: grep the row list). (4) `cargo xtask records new decision D-NEW --series D-10 --story LIBM-F128-EXPLOG-2`
naming the method (Tang 1989/1990, Wong-Goto 1994, Brent-Zimmermann 2010) and bounds; drop the WIP subject.
(5) Long sweep once in release (`cargo xtask long-sweeps` or the #[ignore] test) for the brief's proof.
Then -3: log2/log10/log1p/expm1/exp2/exp10 on the same kernels.

**Do not redo.** The design and proof (module doc); the table generation; the unit tests.
