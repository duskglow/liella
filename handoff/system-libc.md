# system-libc: Liella in libc6's place for Aqours (LIELLA-SYSTEM-LIBC-PLAN)

**State:** branch `task/liella-system-libc-plan` (cut from origin/next `0724bfffa`), role keke
`~/w/n3`. Planning only; no product code. Products: `plan/system-libc.md` (the plan),
`results/system-libc-scan-2026-11-14.md` (raw numbers), `records/pending/LIELLA-SYSTEM-LIBC-PLAN-1.toml`
(D-X pending: scan method). Report json: `handoff/reports/LIELLA-SYSTEM-LIBC-PLAN.json`.

## Results (mirror: owner's LAN Devuan mirror, excalibur + -security + -updates, amd64)

* 70082 binary packages. libc6 lower bound ≤ 2.39: 23171; **2.40: 0**; **2.41: 37** (13 sources,
  7 binaries glibc's own); unversioned 4; none 46870.
* Non-glibc 2.41 dependents (30 binaries, ELF imports read for all):
  * `sched_[gs]etattr`: util-linux (Essential), rt-tests, rt-app, isochron;
  * `sinpi`/`cospi`/`tanpi`/`atanpi`: r-base;
  * `GLIBC_PRIVATE`: libnss-db, unscd, wcc, zzuf;
  * no new symbol (build-time pins): mono (17), firebuild (2), quickroute-gps.
* glibc 2.41 exports = Liella's 2.39 table (`spec/abi/versions.tsv`) + 98, none removed: libc
  `sched_getattr`/`sched_setattr`; libm 40 at GLIBC_2.40 (`exp2m1`, `exp10m1`, `log2p1`,
  `log10p1`, `logp1`) and 56 at GLIBC_2.41 (`sinpi` … `atan2pi`), each in 8 type forms. No
  package imports a GLIBC_2.40 symbol.
* The libc.so.6 `GLIBC_PRIVATE` set equals the table's 278 names.
* Not covered by the table: `libmvec.so.1` (216 exports).
* Gaps a system root needs beyond the symbols:
  * no C header set (liella-gcc uses the host's glibc headers);
  * no `crti.o`/`crtn.o`;
  * no `libresolv.so.2`, `libthread_db.so.1`, `libBrokenLocale.so.1`;
  * no `ldconfig` writer;
  * none of libc-bin's programs;
  * localedef and the data question.

## Jobs (keke)

* Scan (evidence): `lslp-scan-index`, `lslp-scan-debs`, `lslp-scan-meta`, all rc 0.
* Lint/gate: `lslp-lint-1` superseded (records shape: view `x` has only id/body, body one line;
  fixed), then the final jobs named in the report json.
* Scripts and data: `~/work/LIELLA-SYSTEM-LIBC-PLAN/{scan.py,meta.sh,data/}` on keke. Local copy
  in the agent scratch `/home/claude/w/.scratch/LIELLA-SYSTEM-LIBC-PLAN/`. Not committed: they name
  the LAN mirror. Rerun: `python3 scan.py data <keyring> versions.tsv index|debs|sample 100`.

## RULING NEEDED (owner; recommendations in plan §5)

* R1 package identity (recommend glibc's binary names, `Source: liella`).
* R2 versions `<abi>+liella<YYYYMMDD>.<n>-<rev>`, no epoch, symbols minimum = node level.
* R3 2.40/2.41: options with numbers in plan §4 (stay, all 98, or mix: `sched_[gs]etattr` + double
  `sinpi`/`cospi`/`tanpi`/`atanpi`).
* R4 header provenance (generate from `spec/abi` + standards text).
* R5 locale/charmap/translation data (ship Devuan's `locales`/`libc-l10n` data, compiled by
  Liella's localedef).
* R6 utility programs into spec scope.
* R7 zic/zdump/tzselect source.
* R8 drop nscd/unscd.

## Not to redo / cautions

* The mirror scan: rerun only when the mirror moves (`Valid-Until` is weekly) or Devuan's libc6
  changes.
* Maintainer-script bodies of glibc's packages were deliberately not read (names and sizes only).
  Liella's own scripts come from Debian Policy and deb-triggers(5).
* r-base's own math code was not read (it is a math library). Its configure fallback is NOT
  VERIFIED; SL-BOOTSTRAP checks it by building.

## Next step

Owner rules R1–R8. Then brief, in order: SL-FLAVOUR, SL-SYMBOLS, SL-CRT, SL-SONAMES (parallel; DL-8
is the shared prerequisite), then SL-LDCONFIG, SL-BIN-CORE, SL-SWAP-SMOKE (plan §6). Plan rows to
read: plan §1, §3, §6; D-27.9, D-27.16, D-27.28, R14-12; RELEASE-PACKAGING-DEB's records.
