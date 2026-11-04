# ARCH-STRING-PORTABLE (A-8): portable SWAR scanners - run 2 (ARCH-STRING-PORTABLE-2)

State: branch task/arch-string-portable-2 (stacks on task/arch-string-portable), HEAD 397b3592 (+ this report commit).
Role shiki:~/w/sh2. Plan row A-8 (plan/06-phases/20-post-release-ledger-and-experiments.md); decision D-4.22 amendment
PORTABLE-SCAN (plan/07-decisions/s04-byte-arrays-and-strings.md).

## Done (run 1, cc21d5e1)
strz.rs `mod portable` (cfg liella_portable_scan: non-x86_64 or test feature `portable-scan`): strlen, strnlen,
rawmemchr, memchr, memrchr, strchr, strchrnul, strcmp, strncmp (+ entries); ffi.rs cfg swap; build.rs cfg.

## Done (run 2)
* Ruling #2561: memchr gate (`liella_machine_chr_limit`) stays 0 under the cfg (dispatch.rs); the two memchr NULL-fold
  tests expect the S6 abort and the untaken decision there (`selftest::portable_scan()`).
* Ruling #2562: word read stays UNSAFE(slice); new test
  `every_scanner_reads_only_the_pages_of_an_object_at_either_page_edge` (all 9 scanners, both page edges, len 1..=24).
* D-4.22 PORTABLE-SCAN amendment; docs/arch-seams.md A-8 progress paragraph.
* static fixes (397b3592): xtask/src/arch.rs ALLOWED gets strz.rs [machine-decl] (reason in the table); the word read's
  SAFETY comment carries UNSAFE(slice).

## Results
* On dc1f0009 (superseded only by the comment/xtask-only commit 397b3592): arch-string-portable-2-portable rc 0
  (machine_scanners 48 passed, small_copy 9 passed, forced portable); arch-string-portable-2-plants rc 0, all 6 CAUGHT:
  zeros-mask (machine_scanners:798/844 panics), scan-n-tail (SIGABRT), scan-read-ahead (SIGSEGV, page end),
  memrchr-head (SIGABRT), order-terminator (SIGSEGV), scan-n-unaligned (SIGSEGV). No WARN lines (own script, not plant.sh).
  arch-string-portable-2-cross rc 0. arch-string-portable-2-static rc 1 (arch-audit 13 seams, unsafe-audit strz.rs:1853):
  fixed in 397b3592.
* On 397b3592, started detached (not waited, budget): arch-string-portable-2-{static,cross,portable,gate}-r2 (chain
  ~/work/arch-string-portable-2/chain3.sh, sequential) and arch-string-portable-2-plants-r2 (RAM copy
  /dev/shm/claude/arch-string-portable-2/plants-2). Read them in shiki ~/logs/jobs.tsv.

## Open / RULING NEEDED
* The arch-audit allowance covers all of strz.rs for machine-decl (precise today: no other `fn liella_machine_` there).
  Alternative: move `mod portable` to its own file and allow only that (cleaner; needs a file split).

## Not done (scope left)
strcpy/stpcpy/strncpy/stpncpy and _chk, strdiff, casecmp_ord_*, pairscan(_ci), strpair, wcspair, env_slot; the
shifted-word strcmp path (different offsets) is bytewise for now.

## Next step
If any -r2 job is red, fix and rerun only it. Then the copy family portable versions, same test/plant pattern
(plants script: shiki ~/work/arch-string-portable-2/plants.sh).
