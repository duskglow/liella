# System-libc mirror scan, 2026-11-14 (LIELLA-SYSTEM-LIBC-PLAN)

Raw numbers for `plan/system-libc.md`. Source: the project's Devuan mirror (liella-ops
`state/MIRRORS.md`), suites `excalibur`, `excalibur-security`, `excalibur-updates`, components
`main contrib non-free non-free-firmware`, `binary-amd64/Packages.xz`. Only package metadata, file
lists, control files (shlibs, symbols, triggers, conffiles; maintainer scripts by name and size
only) and dynamic symbol tables (`readelf --dyn-syms`) were read. No source code was read.

## Method and trust

* Each suite's `InRelease` verified with `gpgv` and the pinned Devuan keyring: "Good signature from
  Devuan Release Signing (Excalibur)", key `9F8D6C74DE661075FD171BE3B3982868D104092C`. Index dates:
  excalibur 13 Nov 2026 03:24 UTC, -security 13 Nov 16:34 UTC, -updates 13 Nov 20:50 UTC.
* Every `Packages.xz` matched the SHA256 its `InRelease` lists; every `.deb` fetched matched the
  SHA256 in `Packages`.
* Stanzas: excalibur main 68891, contrib 303, non-free 741, non-free-firmware 44; -security main
  2370, contrib 16, non-free-firmware 1; -updates main 3. Merged by package name (a later suite's
  stanza replaces excalibur's: -security and -updates carry newer versions): **70082 binary
  packages**.
* A package's libc6 relation is read from `Pre-Depends` and `Depends`, any alternative named
  `libc6`. Its "need" is the highest lower bound (`>=`, `>>`, `=`) on libc6, upstream part only.
* Jobs (Zen 1 build host): `lslp-scan-index` (indices), `lslp-scan-debs` (glibc's binaries, their
  symbols; the sample of consumers), `lslp-scan-meta` (control files, export tables).

## 1a. Binary packages by libc6 dependency

| libc6 relation | packages | of which `Architecture: all` |
|---|---:|---:|
| none | 46870 | 34098 |
| unversioned `libc6` | 4 | n/a |
| lower bound <= 2.39 | 23171 | 61 |
| lower bound 2.40 | **0** | 0 |
| lower bound 2.41 | **37** | 17 |
| total | 70082 | |

By component: main <= 2.39 22816, 2.41 35, none 46141, unversioned 2; contrib <= 2.39 127, none
176; non-free <= 2.39 228, 2.41 2, none 509, unversioned 2; non-free-firmware none 44.

Lower bounds of the 23208 versioned dependents (packages):

| 2.2 | 2.3 | 2.4 | 2.6 | 2.7 | 2.8 | 2.9 | 2.10 | 2.11 | 2.14 | 2.15 | 2.16 | 2.17 | 2.18 | 2.22 |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 953 | 152 | 2007 | 5 | 115 | 18 | 7 | 3 | 23 | 3395 | 139 | 3 | 84 | 1 | 10 |

| 2.25 | 2.26 | 2.27 | 2.28 | 2.29 | 2.30 | 2.31 | 2.32 | 2.33 | 2.34 | 2.35 | 2.36 | 2.37 | 2.38 | 2.39 | 2.41 |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 25 | 5 | 72 | 31 | 647 | 1 | 1 | 433 | 519 | 6983 | 175 | 22 | 2 | 7169 | 171 | 37 |

Upper-bounded relations (pinned to one glibc release, `libc6 (>> 2.41), libc6 (<< 2.42)` or `=`):
glibc's own `libc-bin`, `libc-dev-bin`, `libc6-dbg`, `libc6-dev`, `libc6-i386`, `libc6-x32`,
`nscd`, and four others: `libnss-db`, `unscd`, `wcc`, `zzuf` (the `GLIBC_PRIVATE` template of
libc6's symbols file).

### Source packages behind the > 2.39 dependents (all 13; fewer than the 50 asked)

Reverse dependencies = binary packages of other sources whose `Depends`/`Pre-Depends` name one of
the source's binaries (direct only).

| source | need | rdeps | binaries > 2.39 | why (from the consumer sample below) |
|---|---|---:|---:|---|
| glibc | 2.41 | 23525 | 7 | its own packages |
| r-base | 2.41 | 425 | 2 | `sinpi`, `cospi`, `tanpi`, `atanpi` @GLIBC_2.41 (`r-base-core`, `r-mathlib`) |
| util-linux | 2.41 | 315 | 1 | `sched_getattr`, `sched_setattr` @GLIBC_2.41 |
| mono | 2.41 | 16 | 17 | no > 2.39 symbol (16 `Architecture: all` CIL packages + `mono-devel`, etc.): build-time pin |
| libnss-db | 2.41 | 1 | 1 | `_nss_files_parse_{etherent,grent,protoent,pwent,rpcent,servent,spent}`, `_nss_netgroup_parseline` @GLIBC_PRIVATE |
| firebuild | 2.41 | 0 | 2 | no > 2.39 symbol: build-time pin |
| isochron | 2.41 | 0 | 1 | `sched_setattr` @GLIBC_2.41 |
| quickroute-gps | 2.41 | 0 | 1 | no > 2.39 symbol: build-time pin |
| rt-app | 2.41 | 0 | 1 | `sched_setattr` @GLIBC_2.41 |
| rt-tests | 2.41 | 0 | 1 | `sched_getattr`, `sched_setattr` @GLIBC_2.41 |
| unscd | 2.41 | 0 | 1 | `__nss_disable_nscd` @GLIBC_PRIVATE |
| wcc | 2.41 | 0 | 1 | `__libc_ifunc_impl_list` @GLIBC_PRIVATE |
| zzuf | 2.41 | 0 | 1 | `__fseeko64` @GLIBC_PRIVATE |

**Consumer sample:** all 30 binaries of the 12 non-glibc sources were fetched and every ELF file's
undefined dynamic symbols at a version above `GLIBC_2.39` or at `GLIBC_PRIVATE` listed. 20 import
none (the build-time pins above). Imports: `sched_setattr` 4 packages, `sched_getattr` 2,
`sinpi`/`cospi`/`tanpi`/`atanpi` 2 each (both r-base), each `GLIBC_PRIVATE` name 1. **No binary in
the archive imports a `GLIBC_2.40` symbol** (one would carry a 2.40 lower bound; there are none).

## 1b/1c. Symbols above 2.39 in glibc 2.41 (libc6 2.41-12+deb13u4)

From the dynamic symbol tables of every shared object in `libc6` (20 libraries, 253 gconv modules),
compared with Liella's export table `spec/abi/versions.tsv` (the 2.39 target):

* **Public exports: 2.41 = Liella's 2.39 table + 98 symbols, none removed**, in every library the
  table covers (`ld-linux-x86-64.so.2`, `libBrokenLocale`, `libanl`, `libc`, `libc_malloc_debug`,
  `libdl`, `libm`, `libnsl`, `libpthread`, `libresolv`, `librt`, `libthread_db`, `libutil`).
* `libc.so.6`, node `GLIBC_2.41` (2): `sched_getattr`, `sched_setattr`. Liella: missing.
* `libm.so.6`, node `GLIBC_2.40` (40): `exp2m1`, `exp10m1`, `log2p1`, `log10p1`, `logp1`, each as
  `<f>`, `<f>f`, `<f>l`, `<f>f32`, `<f>f32x`, `<f>f64`, `<f>f64x`, `<f>f128`
  (`exp2m1f`, `exp2m1f32`, `exp10m1f`, `exp10m1f32` are `IFUNC`). Liella: all missing.
* `libm.so.6`, node `GLIBC_2.41` (56): `sinpi`, `cospi`, `tanpi`, `asinpi`, `acospi`, `atanpi`,
  `atan2pi`, each in the same eight forms. Liella: all missing.
* No other library has a node above 2.39. The libc6 symbols file lists 41 entries at `GLIBC_2.40`
  and 60 at `GLIBC_2.41` (its per-symbol minimum versions); its `shlibs` file says
  `libc6 (>= 2.41)` for every library.
* `GLIBC_PRIVATE`: `libc.so.6` 278 names, identical to the table's 278; `ld-linux-x86-64.so.2` 24
  (table 25, one gone); `libnsl.so.1` 6 = 6; `libresolv.so.2` 2 = 2. Liella's shared products export
  no `GLIBC_PRIVATE` name (D-27.9, R14-12).
* Libraries libc6 ships that the table does not cover: **`libmvec.so.1`** (216 exports, all at
  `GLIBC_2.22`/`GLIBC_2.35`, i.e. part of the 2.39 ABI too), `libnss_compat.so.2` (15
  `GLIBC_PRIVATE`), `libnss_hesiod.so.2` (17 `GLIBC_PRIVATE`), `libnss_files.so.2` and
  `libnss_dns.so.2` (no versioned exports), `libmemusage.so`, `libpcprofile.so` (preload tools).
* Liella's products today (`xtask/src/so.rs`, `tools/deb/build.sh`): `ld-linux-x86-64.so.2`,
  `libc.so.6`, `libm.so.6`, placeholders `libpthread.so.0`, `libdl.so.2`, `librt.so.1`,
  `libutil.so.1`, `libanl.so.1`, `libc_malloc_debug.so.0`, and `libnsl.so.1`; no `libresolv.so.2`
  (its symbols are in `libc.so.6`, D-16.6, but `-lresolv` programs need the soname),
  `libthread_db.so.1`, `libBrokenLocale.so.1`, `libmvec.so.1` or `libnss_*.so.2`. libm exports 127
  of the table's 143 `*f128` names (bincompat export list).

## 2. What glibc's Devuan packages ship (2.41-12+deb13u4, file counts from `dpkg-deb -c`)

| package | Arch | Priority / Essential | Provides | files | control files |
|---|---|---|---|---:|---|
| libc6 | amd64, M-A same | optional | (none; Replaces libc6-amd64; Breaks 30 packages) | 300 | conffiles, preinst, postinst, postrm, templates, shlibs, symbols, triggers |
| libc-bin | amd64, M-A foreign | required, **Essential: yes** | none | 59 | conffiles, postinst, triggers |
| libc6-dev | amd64, M-A same | optional | `libc-dev (= 2.41-12+deb13u4)` | 559 (505 headers) | none |
| libc-dev-bin | amd64 | optional | none | 13 | none |
| libc-devtools | amd64 | optional | none | 20 | none |
| libc-l10n | all | standard | none | 126 | none |
| locales | all | standard | none | 647 | conffiles, config, templates, postinst, prerm, postrm |
| locales-all | amd64 | optional | none | 7122 | postinst, prerm |
| nscd | amd64 | optional (Conflicts unscd) | none | 28 | conffiles, preinst, postinst, prerm, postrm |
| libc6-dbg | amd64 | optional | `libc-dbg, libc6-dbgsym` | 449 | none |
| glibc-doc, glibc-source | all | optional | | 56, 403 | |
| libc6-i386, libc6-x32, libc6-dev-i386, libc6-dev-x32 | multilib | (not inventoried) | | | |

Relations: `libc6` Depends `libgcc-s1`; `libc-bin` Depends `libc6 (>> 2.41), libc6 (<< 2.42)`;
`libc6-dev` Depends `libc6 (=)`, `libc-dev-bin (=)`, `linux-libc-dev`, `libcrypt-dev`,
`rpcsvc-proto` (Breaks 105 packages); `locales` Depends `libc-bin (>> 2.41)`, `libc-l10n (>> 2.41)`,
debconf. Triggers: `libc6` `activate-noawait ldconfig`; `libc-bin` `interest-await ldconfig`.
Conffiles: libc6 `/etc/ld.so.conf.d/x86_64-linux-gnu.conf`; libc-bin `/etc/bindresvport.blacklist`,
`/etc/default/nss`, `/etc/gai.conf`, `/etc/ld.so.conf`, `/etc/ld.so.conf.d/libc.conf`; locales
`/etc/locale.alias`; nscd `/etc/init.d/nscd`, `/etc/nscd.conf`.

File groups (paths without the leading `./`):

* **libc6:** `usr/lib/x86_64-linux-gnu/` `ld-linux-x86-64.so.2`, `libc.so.6`, `libm.so.6`,
  `libmvec.so.1`, `libpthread.so.0`, `libdl.so.2`, `librt.so.1`, `libutil.so.1`, `libanl.so.1`,
  `libresolv.so.2`, `libnsl.so.1`, `libthread_db.so.1`, `libBrokenLocale.so.1`,
  `libc_malloc_debug.so.0`, `libmemusage.so`, `libpcprofile.so`, `libnss_{compat,dns,files,hesiod}.so.2`;
  `usr/lib64/ld-linux-x86-64.so.2 -> ../lib/x86_64-linux-gnu/ld-linux-x86-64.so.2`;
  `usr/lib/x86_64-linux-gnu/gconv/` (247 charset modules, 6 helper libraries `libCNS`, `libGB`,
  `libISOIR165`, `libJIS`, `libJISX0213`, `libKSC`, `gconv-modules`, `gconv-modules.cache`,
  `gconv-modules.d/gconv-modules-extra.conf`).
* **libc-bin:** `usr/bin/` `getconf`, `getent`, `iconv`, `ldd`, `locale`, `localedef`, `pldd`,
  `tzselect`, `zdump`, `ld.so -> ../lib64/ld-linux-x86-64.so.2`; `usr/sbin/` `ldconfig`,
  `iconvconfig`, `zic`; `usr/lib/locale/C.utf8/` (12 category files);
  `usr/share/libc-bin/nsswitch.conf`; the conffiles above.
* **libc6-dev:** 505 headers (`usr/include/`, 324 of them under `usr/include/x86_64-linux-gnu/`,
  plus `usr/include/finclude/x86_64-linux-gnu/math-vector-fortran.h`); `usr/lib/x86_64-linux-gnu/`
  `crt1.o`, `Scrt1.o`, `rcrt1.o`, `crti.o`, `crtn.o`, `gcrt1.o`, `grcrt1.o`, `Mcrt1.o`, `libc.so`
  (linker script), `libm.so` (linker script), `libc.a`, `libc_nonshared.a`, `libm.a`, `libm-2.41.a`,
  `libmvec.a`, `libpthread.a`, `libpthread_nonshared.a`, `libdl.a`, `librt.a`, `libutil.a`,
  `libanl.a`, `libresolv.a`, `libBrokenLocale.a`, `libg.a`, `libmcheck.a`, the `.so` development
  links; `usr/share/gdb/auto-load/.../libc.so.6-gdb.py`.
* **libc-dev-bin:** `usr/bin/gencat`. **libc-devtools:** `memusage`, `memusagestat`, `mtrace`,
  `sotruss`, `sprof`.
* **locales:** `usr/share/i18n/locales/` (371), `usr/share/i18n/charmaps/` (233), `usr/sbin/locale-gen`,
  `update-locale`, `validlocale`, `etc/locale.alias`, `usr/share/locale/locale.alias`.
  **locales-all:** compiled locales under `usr/lib/locale/<name>/` (12 files each).
  **libc-l10n:** `usr/share/locale/<lang>/LC_MESSAGES/libc.mo` (message translations).
* **nscd:** `usr/sbin/nscd`, its init script, `etc/nscd.conf`, a tmpfiles entry.
* **libc6-dbg:** 273 detached debug files under `usr/lib/debug/.build-id/`.
