# XTASK-SMALL-FIXES-1110: four small tooling defects (11-10)

State: branch `task/xtask-small-fixes-1110` (from origin/next 628d4406f). Code head 894b8cfbf; 90a54c9e7 changes only
the plants file; this note and handoff/reports/XTASK-SMALL-FIXES-1110.json sit on top. Role mei:~/w/s1.

## What was done
1. #4293 codemap macro labels (d106e3213, regen 894b8cfbf): `xtask/src/codemap.rs` reads `.S` labels after a small
   object-like `#define` pass (`preprocess`). It runs once per product -D set from crates/boundary/build.rs (libc.so.6:
   EXPORT+RUNTIME, libm.so.6: LIBM, ld.so: LD). Only the `#define`s follow `#if`; code lines are all kept, so line numbers and
   the old label set do not change. symbols.tsv: `SQRTL`/`SQRTF64X ... SLOW` became `sqrtl`/`sqrtf64x ... liella_slow_sqrtl`
   (the only diff). Test `codemap::tests::macro_labels_give_the_exported_name`.
   NOTE: d106e3213 also carries the tools/hottext/regen.sh mode change (item 4). `git update-index --chmod` had staged it
   before the commit, and the commit was already pushed, so it was not split (no force-push).
2. #4338 affected fuzz/ (1c25cdadd): the `fuzz/` rule now comes before the build-configuration rule. `fuzz/Cargo.lock`
   and `fuzz/rust-toolchain.toml` also select `fuzz-check`. validation/ is unchanged. Test
   `affected::tests::fuzz_workspace_files_select_only_fuzz_check`.
3. #4218 (65be3f422): both tests were spectest PENDING STUBS. They are ignored by design, and `pending()` panics, so
   any `--include-ignored` run fails them. They are now real tests of the same name: `fam_csqrt` =
   `products::math_family("csqrt") == 8`. Each `sym___sqrt{,f,l,f128}_finite` calls the new `products::math_compat`:
   libm.so.6 exports the name at its non-default version only, and libm.a does not define it. The jump target is
   still covered by s10_0_c2_0.
4. #4264/#4328: regen.sh is 100755 in git. sync_role needs NO change: its rsync uses `-rlpD` (-p keeps modes) and the tar
   fallback keeps modes too. The bit was lost only because git checked the file out as 644.

## Results (mei, s1)
- Clippy and static: xsf1110-static rc 0 on 894b8cfbf. Earlier, xsf1110-checks was red only on `codemap out of date`,
  which 894b8cfbf fixes.
- xtask tests: xsf1110-xtests rc 0 (6 passed: codemap:: plus affected::fuzz).
- Codemap regeneration: xsf1110-codemap rc 0.
- Gate: xsf1110-gate (`affected --run --base origin/next`) was STILL RUNNING at hand-back. Re-attach:
  `~/bin/jobwait xsf1110-gate` on mei.
- Plants: NOT RUN. xsf1110-plants and xsf1110-plants-2 were refused (rc 3), because `.S` plants must anchor on a
  `liella_machine_` label and compat_aliases.S has none. 90a54c9e7 swaps them for two helper plants. That file could
  not be synced: sync_role reported BUSY because the gate was running. The s10_math_symbols tests themselves have not
  run yet; the gate selects them.

## Next step
After xsf1110-gate ends: sync 90a54c9e7, then run
`cargo xtask plants handoff/XTASK-SMALL-FIXES-1110-plants.txt --dir /dev/shm/claude/xsf1110-plants-3 --timeout 900`
(8 plants). Then `cargo test -p liella-boundary --test s10_math_symbols -- fam_csqrt sym___sqrt` (products built).

## Rulings / informational
- RULING NEEDED: 168 other family!/symbol! stubs in s10_math_symbols.rs (and stubs elsewhere) fail under
  `--include-ignored` the same way. Recommendation: no gate runs `--include-ignored` across stub files; or `pending()`
  prints and returns, with the ignore reason as the marker.
- IMPROVEMENT: `cargo xtask plants` refuses every `.S` plant without a `liella_machine_` anchor, even in generated
  files such as compat_aliases.S whose labels are `liella_compat_*`. It should accept any named label anchor.
