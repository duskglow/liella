# ARCH-STRING-PORTABLE (A-8): portable SWAR scanners - CHECKPOINT, NOT READY

State: branch task/arch-string-portable, WIP code commit cc21d5e1 (from origin/next). Role shiki:~/w/sh2.
Plan row: plan/06-phases/20-post-release-ledger-and-experiments.md A-8. No D-row written yet (use D-NEW).

## Done
* crates/boundary/src/strz.rs, mod portable (cfg(liella_portable_scan)): SWAR strlen, strnlen, rawmemchr(+_entry),
  memchr(+_entry), memrchr(+_entry), strchr, strchrnul, strcmp(+_entry, extern "C" because selftest passes it to
  clean_upper_call), strncmp. Exact per-byte zero mask (works on both endians), aligned word loads only
  (read_volatile), head/tail bytes masked. strcmp/strncmp use words when both strings have the same offset in a
  word, bytes otherwise (a follow-up can add the shifted-word path).
* ffi.rs: the 13 declarations carry cfg(not(liella_portable_scan)); a cfg'd pub(crate) use takes the portable ones.
* build.rs: emits cfg liella_portable_scan on non-x86_64 or with feature portable-scan (Cargo.toml, test-only).

## Results (foreground on sh2, not jobrun)
* cargo test -p liella-boundary --features portable-scan --test machine_scanners: 45 passed, 2 FAILED:
  memchr_answers_alike_with_null_folding_on_and_off_and_takes_the_folding_decision (line 1988),
  null_faults_in_the_folded_memchr_sizes_and_aborts_in_every_other (line 1771). Cause: with folding on the asm
  memchr entry faults on page zero for NULL; the portable entry sends NULL to find_n_edge (abort), since a NULL read
  is UB in Rust. small_copy did not run (cargo stopped at the first failing target).
* NOT RUN: default suites, small_copy forced, plants, cargo xtask clippy/static, gate. Unknown whether the two
  "never used" warnings in the log are pre-existing.

## RULING NEEDED
1. NULL folding on portable targets: recommend the dispatcher reports folding OFF whenever liella_portable_scan is set
   (so every path aborts alike), and the two tests expect that decision under portable-scan.
2. Unsafe category: the aligned word over-read sits under UNSAFE(slice) in strz.rs (no policy change). Recommend
   the owner confirm, or rule a dedicated category/file for word scans.

## Not done (scope left)
strcpy/stpcpy/strncpy/stpncpy and the _chk forms, strdiff, casecmp_ord_*, pairscan(_ci), strpair, wcspair,
env_slot (docs/arch-seams.md list); docs/arch-seams.md and the D-row are not updated.

## Next step
Settle ruling 1, then: forced and default runs of machine_scanners + small_copy; plants in a RAM copy (zeros() drop
"| x"; scan_n below(rem) -> below(rem+1); scan reading the next word before the test; memrchr from(s-q) -> from(s-q-1);
order() dropping zeros(x)); cargo xtask clippy and static; gate via jobrun. Then the remaining kernels.
Note: one edit to strz.rs was done with a python rewrite instead of the Edit tool (rule breach, reported).
