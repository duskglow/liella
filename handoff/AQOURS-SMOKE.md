# AQOURS-SMOKE: Debian programs built from source against Liella (harness + findings)

**State:** branch `task/aqours-smoke` (cut from origin/next `d01086da3`), role kanon `~/w/kn1`. Head: the commit
that adds this note (code head is the one before it). Full results: `validation/smoke/RESULTS.md`; harness:
`validation/smoke/README.md`. No Liella code changed (a harness and a report, as the brief asks).

## Done
* `validation/smoke/`: `smoke.sh` (one command; private apt for trixie sources from deb.debian.org with the
  Debian archive key; reference and Liella builds; test-suite counts; per-test comparison from `.trs`),
  `liella-cc` (the conformance dynamic link line as CC/CXX), `check-elf.sh` (every built object must be on
  Liella's loader and libc), `smoke.sh selftest` with one negative control per rule, `programs.sh` (recipes),
  `repro/` (three reproducers).
* Ran: zlib, xz-utils, sqlite3, grep, sed, diffutils, findutils, coreutils, gawk, bash, perl (miniperl),
  python3.13 (sample), cppunit. Not built: make-dfsg (harness, F11), lua5.4 (no libtool on host, F10).
* Plants `handoff/AQOURS-SMOKE-plants.txt`: 6 CAUGHT (`aqours-smoke-plants4`), no WARN.

## Jobs (kanon)
* Final: `aqours-smoke-lint2` (clippy + static), `aqours-smoke-gate2` (affected, base origin/next: 0 tests reach
  validation/), `aqours-smoke-plants4` (6/6 CAUGHT on the harness files of the final head).
* Smoke runs (evidence, not gates): `aqours-smoke-run1` (zlib, xz, sqlite3), `-run2` (sqlite3 ... bash),
  `-run3`/`-run4` (perl, python3, cppunit), `-run5`, `-run7` (coreutils, gawk, make, grep on the last harness).
  Work dirs `~/work/AQOURS-SMOKE/smoke*`, products copy `~/work/AQOURS-SMOKE/products` (release profile).
* Superseded: `aqours-smoke-self1` (libm link), `-plants1` (soname control not isolated), `-plants2` (before the
  `-x none` fix), `-plants3`/`-run6` (stopped: started on a stale sync), `-gate1`/`-lint1` (older head).

## Findings (classes: RESULTS.md)
* (a) F1 `readdir` on a removed directory sets ENOENT (spec/behaviour/fstree.md says errno 0):
  `validation/smoke/repro/readdir_removed.c`. F2 `regcomp` refuses >= 256 nested groups (REG_ESPACE), no
  differences row: `repro/regcomp_nesting.c`. F3 failed `setlocale` leaves errno 0 (reference ENOENT):
  `repro/setlocale_errno.c`.
* (b) dlsym/dlvsym DL-16, dlclose DL-17, dladdr/dlinfo DL-18, LD_PRELOAD DL-21, gdb `_r_debug` DL-23, start-time
  soname search DL-8 (not on the start path yet).
* (c) no chroot: kanon lacks `uidmap` (newuidmap) and `mmdebstrap`; no autotools/libtool/cmake on the host.

## Next step
1. Owner: `sudo apt-get install uidmap mmdebstrap` on kanon (or run the next smoke story on a host that has
   them), then add a `--chroot` mode (mmdebstrap --mode=unshare, `apt-get build-dep`, dpkg-buildpackage) so
   lua5.4, make and the full Python suite build as Debian builds them.
2. make-dfsg recipe: `make -C lib` before `make make`; rerun `smoke.sh make`.
3. Rerun the whole list when DL-16/DL-8 land (re-enable sqlite3 extensions, Python shared modules, a plugin
   program) and when F1-F3 are decided.
