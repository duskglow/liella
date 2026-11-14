# Liella as the system C library of Aqours (LIELLA-SYSTEM-LIBC-PLAN)

Owner ruling "Liella as the system libc for Aqours" (2026-11-13): Aqours, the owner's Devuan-based
distribution, installs no glibc. It rebuilds from source against Liella (route A), and Liella's
packages take over libc6's role: paths, libc-bin's tools, data. RELEASE-PACKAGING-DEB built
`liella6`/`liella6-dev`, which **coexist** with glibc under `/usr/lib/liella`. This plan covers the
step after that: Liella **in place of** libc6. It is a plan only, with no product code. The raw
numbers are in `results/system-libc-scan-2026-11-14.md`.

Items marked **RULING NEEDED** are for the owner. §4 is decision input with numbers, not a decision.

## 1. What the scan says (Devuan excalibur amd64, 70082 binary packages)

* Packages that depend on libc6: 23171 need ≤ 2.39, **0 need 2.40, 37 need 2.41**, 4 unversioned,
  and 46870 have no libc6 relation (34098 of these are `Architecture: all`).
* The 37 packages that need 2.41 come from 13 source packages. Seven are glibc's own. The other 30
  fall into four groups (each consumer's ELF imports were read):
  * **`sched_getattr`/`sched_setattr` @GLIBC_2.41** (4 packages): util-linux (Essential; 315
    reverse dependencies), rt-tests, rt-app, isochron.
  * **`sinpi`, `cospi`, `tanpi`, `atanpi` @GLIBC_2.41** (2 packages): r-base (`r-base-core`,
    `r-mathlib`; 425 reverse dependencies).
  * **GLIBC_PRIVATE** (4 packages): libnss-db, unscd, wcc, zzuf. They break with any Liella
    version, because Liella exports no `GLIBC_PRIVATE` name (D-27.9, R14-12).
  * **Build-time pins with no symbol above 2.39** (20 packages): mono (17), firebuild (2),
    quickroute-gps (1). A rebuild against Liella removes the pin.
* No package in the archive imports a `GLIBC_2.40` symbol.
* glibc 2.41's public exports are exactly Liella's 2.39 table plus 98 symbols, with nothing
  removed:
  * libc.so.6: `sched_getattr` and `sched_setattr` (2);
  * libm.so.6, GLIBC_2.40 (40): `exp2m1`, `exp10m1`, `log2p1`, `log10p1`, `logp1`;
  * libm.so.6, GLIBC_2.41 (56): `sinpi`, `cospi`, `tanpi`, `asinpi`, `acospi`, `atanpi`,
    `atan2pi`.

  Each libm name comes in eight forms: plain, `f`, `l`, `f32`, `f32x`, `f64`, `f64x`, `f128`.
  Liella has none of the 98.
* libc6 also ships things Liella has no table row or product for: `libmvec.so.1` (216 exports, all
  at nodes ≤ 2.35), `libresolv.so.2` as a shared object, `libthread_db.so.1`,
  `libBrokenLocale.so.1`, and `libnss_{files,dns,compat,hesiod}.so.2`.

## 2. Inventory: glibc's Devuan packages against Liella

Status: **has** = Liella ships it; **equiv** = Liella has an equivalent; **missing**; **not
needed** (for Aqours).

| item (glibc package) | status | Liella file / phase item / story (§6) |
|---|---|---|
| `ld-linux-x86-64.so.2` (libc6) | has (system paths: missing) | `crates/ld-so`; start-time soname search DL-8 not on the start path yet (AQOURS-SMOKE); prefix `/`: SL-FLAVOUR |
| `libc.so.6`, `libm.so.6` (libc6) | has | `cargo xtask so` |
| `libpthread.so.0`, `libdl.so.2`, `librt.so.1`, `libutil.so.1`, `libanl.so.1`, `libc_malloc_debug.so.0` (libc6) | has (placeholders) | D-27.9 |
| `libnsl.so.1` (libc6) | has | shipped by `tools/deb/build.sh` |
| `libresolv.so.2` (libc6) | missing (symbols are in libc.so.6, D-16.6) | SL-SONAMES |
| `libthread_db.so.1` (libc6) | missing | phase 14 (symcov "phase 14"); SL-SONAMES tracks it |
| `libBrokenLocale.so.1` (libc6) | missing ("not in plan") | SL-SONAMES |
| `libmvec.so.1` (libc6) | missing | M-0/M-1 (ledger). With Liella's own headers (no `simd` declarations) no rebuilt package imports it; foreign binaries only |
| `libnss_files.so.2`, `libnss_dns.so.2` (libc6) | not needed | built into libc (`crates/core/src/names/nss.rs`: `files`, `dns`) |
| `libnss_compat.so.2`, `libnss_hesiod.so.2` (libc6) | missing / not needed | `compat` NSS service: SL-NSS (with M14-12); hesiod: not needed (no Aqours user named) |
| `libmemusage.so`, `libpcprofile.so` (libc6) | missing, later | SL-DEVTOOLS |
| gconv modules, `gconv-modules*`, `.cache` (libc6) | equiv / not needed | iconv reads charmaps (D-6.8, `crates/core/src/charmap`, built-in codecs `crates/core/src/iconv`); no loadable modules |
| `ld.so.conf.d/x86_64-linux-gnu.conf` (libc6 conffile) | missing | SL-FLAVOUR (package content) |
| `ldconfig` (libc-bin) | missing; cache *reader* has | `crates/ld/src/cache.rs` reads format D.3.1; writer + conf parser: SL-LDCONFIG |
| `ldd` (libc-bin) | equiv | the loader's `--list`; `ldd` script: SL-BIN-CORE |
| `getconf`, `getent`, `iconv`, `locale` (libc-bin) | missing (the libc functions exist) | SL-BIN-CORE |
| `localedef` (libc-bin), `locale-gen`, `update-locale`, `validlocale` (locales) | missing | SL-LOCALEDEF (writes D.1 and D.1.6) |
| `zic`, `zdump`, `tzselect` (libc-bin) | missing | SL-TZ (**RULING NEEDED** R7) |
| `pldd`, `iconvconfig` (libc-bin) | missing / not needed | pldd: SL-BIN-CORE; iconvconfig: a no-op (no gconv cache) |
| `usr/lib/locale/C.utf8/*` (libc-bin) | equiv | built-in `C.UTF-8` (`crates/core/src/locale/mod.rs`) |
| `nsswitch.conf` template, `gai.conf`, `bindresvport.blacklist`, `default/nss`, `ld.so.conf`, `ld.so.conf.d/libc.conf` (libc-bin conffiles) | missing (Liella reads nsswitch.conf and gai.conf) | SL-FLAVOUR (package content) |
| ldconfig trigger: libc6 `activate-noawait ldconfig`, libc-bin `interest-await ldconfig` | missing | SL-LDCONFIG |
| 505 headers (libc6-dev) | **missing**: liella-gcc compiles against the host's glibc 2.39 headers | SL-HEADERS (**RULING NEEDED** R4) |
| `crt1.o`, `Scrt1.o`, `rcrt1.o`, `gcrt1.o` (libc6-dev) | has | `liella-crt1.o`, `liella-rcrt1.o`, `liella-gcrt1.o` |
| `crti.o`, `crtn.o`, `grcrt1.o`, `Mcrt1.o` (libc6-dev) | missing | SL-CRT (plain `gcc` needs crti/crtn; liella-gcc links `-nostartfiles`) |
| `libc.so` script, `libc_nonshared.a`, static archives (libc6-dev) | has | D-27.28, build.sh |
| `libm.so` script (`AS_NEEDED` libmvec), `libm-2.41.a`, `libmvec.a`, `libg.a`, `libmcheck.a`, `libpthread_nonshared.a`, `libBrokenLocale.a` (libc6-dev) | partly (`libm.so` is a link) | SL-SONAMES (empty archives where glibc's are) |
| `libc.so.6-gdb.py` (libc6-dev) | not needed | gdb support is DL-23 |
| `gencat` (libc-dev-bin) | missing | SL-DEVTOOLS (format D.2.2 exists) |
| `memusage`, `memusagestat`, `mtrace`, `sotruss`, `sprof` (libc-devtools) | missing | SL-DEVTOOLS (later, not on the boot path) |
| charmaps, locale sources (locales); compiled locales (locales-all) | data | **RULING NEEDED** R5 |
| translations `libc.mo` (libc-l10n) | data | **RULING NEEDED** R5 (Liella's messages are glibc's strings, `spec/abi/errmsgs.tsv`) |
| `nscd` (nscd), `unscd` | not needed | Liella has no nscd client; unscd imports `GLIBC_PRIVATE`: drop (R8) |
| detached debug info (libc6-dbg) | later | build `-dbg` from the same build (SL-FLAVOUR follow-up) |
| multilib `libc6-i386`, `libc6-x32` | not needed | Liella is x86_64 only |

## 3. Packaging for Aqours

### 3.1 Identity (RULING NEEDED R1)

* **Option A (recommended): take glibc's binary package names.** Liella's source package builds
  `libc6`, `libc-bin` (`Essential: yes`, `Priority: required`), `libc6-dev` (`Provides: libc-dev`),
  `libc-dev-bin`, `locales`, `libc-l10n`. Aqours keeps its Devuan base (debootstrap, the Essential
  set, `build-essential`'s `libc6-dev | libc-dev`), and about 23200 dependency lines by name. The
  Liella origin shows in `Source: liella`, in `Maintainer: Liella Maintainers <liella@koiren.ai>`,
  and in the version (§3.2).
  * **Why not Provides:** debootstrap's own resolver does not follow virtual packages (NOT VERIFIED
    in this run; SL-SWAP-SMOKE checks it), and `Essential` belongs to a real package, never to a
    Provides.
* **Option B: keep `liella6`, `liella-bin`, `liella6-dev`** with
  `Provides: libc6 (= <v>), libc-bin, libc-dev`, `Conflicts`/`Replaces` libc6. These names cannot
  be mistaken for glibc. Costs:
  * debootstrap cannot install such a root;
  * every package whose dependency names libc-bin needs the versioned Provides;
  * a package rebuilt on Aqours depends on `liella6` and does not install on Devuan.
* **Either way:**
  * the coexisting `liella6`/`liella6-dev` (sysroot `/usr/lib/liella`) stay, for Liella on
    distributions that keep glibc;
  * the system flavour is a second build of the same tree (SL-FLAVOUR) and does not replace them.
* **Fields:**
  * `Multi-Arch: same` for libc6/libc6-dev, `foreign` for libc-bin;
  * `Breaks: base-files (<< 13.3~)` as libc6 has: the merged-`/usr` links `/lib64 -> usr/lib64`
    come from base-files;
  * no `Breaks` list taken over wholesale. glibc's 30 Breaks name packages fixed for glibc changes;
    Liella's own list starts empty and grows from Aqours' rebuild findings.

### 3.2 Versions, symbols and shlibs (RULING NEEDED R2)

* **Version `<abi>+liella<YYYYMMDD>.<n>-<rev>`, no epoch.**
  * Today `<abi>` is `2.39`. It would become `2.41` if the owner takes the 2.40/2.41 additions.
  * Debian ordering gives `2.39 < 2.39+liella20261201.1 < 2.40`. Every `libc6 (>= 2.x)` dependency
    with x ≤ 39 (23208 packages) is met, and every 2.40/2.41 dependency is honestly not met.
  * An epoch (`1:2.39...`) would falsely meet the 2.41 dependencies of unrebuilt binaries. Those
    binaries then fail at load time with a missing version node, so no epoch.
* **The `symbols` file** is generated from `spec/abi/versions.tsv` and the built objects (proof:
  `dpkg-gensymbols -c4` against the built libraries):
  * one `<library> libc6 #MINVER#` block per library;
  * every `GLIBC_2.x` symbol's minimum version is `2.x`, the ABI level that defines it. A rebuilt
    package therefore still installs on any C library at that level;
  * a symbol whose behaviour Liella changes later in a way programs depend on gets the Liella
    version that changed it;
  * `LIELLA_NATIVE_<n>` symbols get a second dependency template (`| liella-native-abi (>= <n>)`,
    a virtual package that only Liella provides), so a program that uses Liella-only interfaces
    never installs against glibc;
  * there are no `GLIBC_PRIVATE` lines (R14-12).
* **`shlibs`:** `libc 6 libc6 (>= <abi>)` per library, as a fallback for tools that do not read
  symbols files.
* **`__GLIBC__`/`__GLIBC_MINOR__`** in Liella's headers, `gnu_get_libc_version()` and
  `confstr(_CS_GNU_LIBC_VERSION)` keep reporting `<abi>`; configure checks key on them.

### 3.3 Paths: no sysroot

* The loader and libraries are as in the inventory above, under `/usr/lib/x86_64-linux-gnu/`, with
  `/usr/lib64/ld-linux-x86-64.so.2 -> ../lib/x86_64-linux-gnu/ld-linux-x86-64.so.2`. `PT_INTERP`
  is `/lib64/ld-linux-x86-64.so.2`, gcc's default, so plain `gcc` works and liella-gcc is not
  needed on Aqours.
* The build uses the knobs of RELEASE-PACKAGING-DEB's record -2: prefix `/` and
  `LIELLA_LIBDIR=usr/lib/x86_64-linux-gnu`. Whether an empty or `/` prefix builds as it stands is
  NOT VERIFIED (SL-FLAVOUR).
* The loader's default directories already are `/lib/x86_64-linux-gnu`, `/usr/lib/x86_64-linux-gnu`,
  `/lib`, `/usr/lib` (`crates/ld/src/arch.rs`), and it reads `/etc/ld.so.cache`.
* Headers go to `/usr/include` and `/usr/include/x86_64-linux-gnu`; locales to `/usr/lib/locale`
  (`crates/core/src/locale`); charmaps to `/usr/share/i18n/charmaps`; the alias file is
  `/usr/share/locale/locale.alias`. These are the paths Liella already reads.

### 3.4 Programs libc-bin must ship before a root can install packages

libc-bin is `Essential: yes`, so any maintainer script may run its programs without a dependency.
Order:

1. **`ldconfig`** with `/etc/ld.so.conf` (`include` lines), `-p`, `-r`, `-X`, `-N`, `-n`, `-v`,
   `-C`, `-f`. It writes format D.3.1 (`crates/ld/src/cache.rs` reads it back). libc-bin carries
   the `interest-await ldconfig` trigger, and its postinst runs ldconfig on `triggered`. A root
   boots without a cache, because the default directories are searched, but libraries in
   `ld.so.conf.d` directories are not found without one.
2. **`getent`** (adduser and account-creating postinsts), **`getconf`**, **`locale`**, **`iconv`**,
   **`ldd`** (shell front end over the loader's `--list`).
3. **`localedef`** (+ `locale-gen`) as soon as any locale beyond `C`/`C.UTF-8`/`POSIX` is wanted,
   and **`zic`** before tzdata can be built from source.
4. Later: `pldd`, `zdump`, `tzselect`, `iconvconfig` (no-op), `gencat`, the devtools.

**NOT VERIFIED:** which Essential-set maintainer scripts call which program. SL-SWAP-SMOKE greps
the installed root's `/var/lib/dpkg/info/*.{pre,post}{inst,rm}` for the libc-bin program names and
records the list.

**RULING NEEDED R6:** the spec puts the utility programs out of scope (line 10 of
`glibc-functional-spec.md`). A system libc brings them in. Recommendation: add a spec part
"utilities" with clauses from POSIX (getconf, iconv, locale, localedef, gencat), man pages
(ldconfig(8), ldd(1), getent(1), zic(8), zdump(8)) and black-box probes, with formats D.1–D.3 as
the file contracts.

### 3.5 Locale and iconv data (RULING NEEDED R5)

Liella reads compiled locales and `locale-archive` (spec Appendix D.1) and installed charmaps
(D-6.8). Aqours needs the data from somewhere:

* **(a) recommended:** ship Devuan's `locales` and `libc-l10n` data unchanged as Aqours packages.
  * They are `Architecture: all` data plus three scripts that Liella's own `locale-gen` replaces.
    Liella developers do not read them, and reading installed charmaps is what D-6.8 already does.
  * They come from glibc's source package, so the owner rules whether "no glibc at all" covers
    them.
* **(b)** generate locale sources and charmaps from Unicode/CLDR data (L, its own spec gap).
* **(c)** ship only `C`, `C.UTF-8` and `POSIX`, which are built in. This boots, but locales fail.

The compiled locales are then made by Liella's `localedef` (SL-LOCALEDEF), never taken from
locales-all.

### 3.6 NSS

* Liella's NSS is built in: `files` and `dns` (`crates/core/src/names/nss.rs`), plus the in-process
  cache `names/cache.rs`.
* External modules (`libnss-ldapd`, `libnss-mdns` and the like) wait for **M14-12** (module service
  kind, trusted-directory loading). Until then those packages are listed as not working.
* `compat` (`passwd: compat` with `+`/`-` lines) is a service libc6 ships as a module. SL-NSS
  decides built-in vs module.
* `nscd`/`unscd` are dropped (**R8**, recommended): no Liella nscd client exists, and unscd uses
  `GLIBC_PRIVATE`.

### 3.7 Building Aqours against Liella (dpkg-buildpackage path)

1. **Stage 0 (Devuan builder with glibc):** build Liella's system packages (SL-FLAVOUR) and a local
   apt repository holding them.
2. **Stage 1, swap (SL-SWAP-SMOKE):**
   * mmdebstrap an excalibur root; add the Liella repository with `Pin-Priority: 1001` (the
     versions sort below 2.41, so the swap is a pinned downgrade);
   * replace `libc6`, `libc-bin`, `libc6-dev`, `libc-dev-bin`;
   * scan every installed ELF's version needs. Expected from §1: in the base set only
     util-linux's 2.41 symbols block, apt refuses util-linux (`libc6 (>= 2.41)`), and Essential
     util-linux must be rebuilt in stage 0 against Liella first (or SCHED-ATTR lands, §4);
   * expected, NOT VERIFIED: gcc, binutils, dpkg, apt and perl are glibc-built binaries that need
     ≤ 2.39 and run on Liella.
3. **Stage 2 (SL-BOOTSTRAP-ESSENTIAL):** in the swapped root, `dpkg-buildpackage` the Essential set
   and build-essential (gcc, binutils, dpkg-dev, make, perl) against Liella's headers and
   libc6-dev. `dpkg-shlibdeps` finds `libc.so.6`'s owner (`libc6`, Liella's) and reads its symbols
   file, so dependencies come out as `libc6 (>= 2.x)` per §3.2. Then rebuild the root from those
   packages alone: the first all-Liella Aqours root.
4. **Stage 3 (Aqours archive):** rebuild in build-dependency order. Each failure is classified as
   Liella defect, Liella missing interface, or package patch. The GLIBC_PRIVATE four are patched
   or dropped. r-base, rt-tests, rt-app and isochron follow §4. The AQOURS-SMOKE findings carry
   over.

## 4. The 2.40/2.41 decision (owner decides; numbers from §1)

| group | symbols | Aqours packages importing them | cost estimate (not measured) |
|---|---:|---|---|
| G1 `sched_getattr`, `sched_setattr` (libc, 2.41) | 2 | util-linux (**Essential**), rt-tests, rt-app, isochron: 4 binaries / 4 sources | **S**: two syscall wrappers; `struct sched_attr` from the kernel UAPI; header declaration; tests |
| G2 C23 `exp2m1`, `exp10m1`, `log2p1`, `log10p1`, `logp1` (libm, 2.40) | 40 | **none** | **L**: `logp1` = `log1p` (C23 7.12.6), aliases only; 4 new functions × 4 formats (float, double, x87 long double, binary128) = 16 kernels to Liella's libm accuracy bar; `f32`…`f128` names are aliases |
| G3 C23 `sinpi` … `atan2pi` (libm, 2.41) | 56 | r-base (`r-base-core`, `r-mathlib`): `sinpi`, `cospi`, `tanpi`, `atanpi` double only | **L–XL**: 7 functions × 4 formats = 28 kernels; the double `sinpi`/`cospi`/`tanpi`/`atanpi` subset alone is **M** |
| shared cost of any addition | | | **M**: a 2.41 reference system for probes (excalibur chroot); spec clauses for the new names (C23, man pages); headers; version nodes `GLIBC_2.40`/`GLIBC_2.41`; `<abi>` in §3.2 becomes 2.41 |
| GLIBC_PRIVATE users | 278 names exist (= table) | libnss-db, unscd, wcc, zzuf | patch or drop in every option (R14-12) |
| build-time pins | 0 | mono (17), firebuild (2), quickroute-gps (1) | none: a rebuild removes the pin |

Options:

1. **Stay at 2.39.**
   * util-linux, rt-tests, rt-app, isochron and r-base must build without the 2.41 functions.
     Whether their configure checks fall back by themselves is **NOT VERIFIED**: a link test
     against a libm/libc without the name would fail and select their own code. SL-BOOTSTRAP
     proves it for util-linux first.
   * Cost: per-package patches where the fallback is missing (S each, 5 sources).
   * Risk: util-linux is Essential and sits on the bootstrap path.
2. **Take all 98 (ABI level 2.41).** Cost G1 + G2 + G3 + shared: about 2 S/M stories plus 44 libm
   kernels.
   * Benefit: unmodified rebuilds of all 13 sources, and `<abi>` 2.41 meets the build-time pins
     without a rebuild.
   * The libm half is used by one package today (4 of 98 symbols).
3. **Mix (recommendation for the owner to weigh):**
   * add **G1** now (S; unblocks the Essential util-linux and three RT tools);
   * add the 2.41 **double** `sinpi`/`cospi`/`tanpi`/`atanpi` (M; r-base);
   * take G2 and the rest of G3 into the normal libm backlog;
   * keep `<abi>` at 2.39 until the `GLIBC_2.41` node is complete. A partial node only matters
     to foreign binaries, which route A does not run, and it is a `differences` row while partial.

## 5. Open rulings (owner)

| id | question | recommendation |
|---|---|---|
| R1 | package identity | Option A: glibc's binary names, `Source: liella` |
| R2 | version scheme | `<abi>+liella<YYYYMMDD>.<n>-<rev>`, no epoch; symbols min version = node level |
| R3 | 2.40/2.41 | owner's choice from §4; option 3 laid out as the cheapest path that unblocks all 13 sources |
| R4 | header provenance: Liella has no header set | generate headers from `spec/abi` (prototypes, constants, layouts, OD-1) plus the standards' text; no glibc header text |
| R5 | locale/charmap/translation data | ship Devuan's `locales`/`libc-l10n` data unchanged, compiled by Liella's localedef |
| R6 | utility programs into scope | yes: a spec "utilities" part (§3.4) |
| R7 | `zic`/`zdump`/`tzselect` | write them clean-room (SL-TZ, M); or a separate Aqours package built from IANA tzcode that Liella developers never read. Owner's call (tzcode also holds a `localtime` implementation) |
| R8 | nscd/unscd | drop from Aqours |

## 6. Stories (in order, so an Aqours root boots and installs packages as early as possible)

| # | story | size | needs | proof |
|---|---|---|---|---|
| 1 | **DL-8 start-time soname search** (existing phase-14 story) | — | — | AQOURS-SMOKE's dynamic programs run |
| 2 | **SL-FLAVOUR**: system build (prefix `/`, multiarch libdir), packages per R1/R2 with conffiles, `/etc/ld.so.conf.d/x86_64-linux-gnu.conf`, triggers | M | R1, R2 | `check.sh`-style package checks; install into an empty root; `PT_INTERP` `/lib64/ld-linux-x86-64.so.2`; lintian |
| 3 | **SL-SYMBOLS**: symbols and shlibs generator from `spec/abi/versions.tsv` | S | R2 | `dpkg-gensymbols -c4` clean against the built libraries; `dpkg-shlibdeps` on 3 sample programs gives the expected relation; plants |
| 4 | **SL-CRT**: `crti.o`, `crtn.o` (psABI `.init`/`.fini`), `grcrt1.o`, `Mcrt1.o` | S | — | plain `gcc` (no `-nostartfiles`) links and runs hello, C++ static ctor, `-pg` |
| 5 | **SL-SONAMES**: `libresolv.so.2`, `libBrokenLocale.so.1` placeholders (D-27.9 rule), empty archives as libc6-dev ships them, `libm.so` script | S | — | readelf vs table; `-lresolv` dynamic program runs |
| 6 | **SL-LDCONFIG**: writer for D.3.1, `ld.so.conf` parser, options of ldconfig(8), libc-bin trigger and postinst | M | R6 | differential: reference loader and Liella loader both resolve via a Liella-written cache; `ldconfig -p` vs reference; fuzzed conf files; plants |
| 7 | **SL-HEADERS**: Liella's header set, split by spec section | XL (split) | R4 | compile the conformance suite and a package corpus against them; layout/constant checks vs `spec/abi` tables |
| 8 | **SL-BIN-CORE**: `getent`, `getconf`, `locale`, `iconv`, `ldd`, `pldd`, `iconvconfig` (no-op) | M | R6 | differential output vs reference on the probe host; exit codes |
| 9 | **SL-SWAP-SMOKE**: excalibur root with Liella swapped in (pinned), ELF version-need scan, maintainer-script program list, `apt install` of 10 packages | M | 1–6, 8 | job log: install/remove pass list; list of blocking binaries (expected: util-linux) |
| 10 | **SCHED-ATTR** (if R3 takes G1) | S | R3 | tests vs kernel UAPI; util-linux `chrt` runs |
| 11 | **SL-BOOTSTRAP-ESSENTIAL**: rebuild Essential + build-essential against Liella, then an all-Liella root | L | 7, 9, (10) | the root debootstraps from the Aqours repository and rebuilds `hello` with dpkg-buildpackage |
| 12 | **SL-LOCALEDEF** (+ `locale-gen`, `update-locale`, `validlocale`) | L | R5, R6 | compiled locales byte-compatible with D.1; `setlocale` of each generated locale; differential `locale -a`/`LC_ALL=x locale -k` |
| 13 | **SL-TZ** (`zic`, `zdump`, `tzselect`) or the R7 alternative | M | R7 | tzdata builds; TZif output (RFC 8536) loads in Liella and the reference |
| 14 | **SL-NSS**: `compat` service; external modules with M14-12 | M | M14-12 | test module + `passwd: compat` smoke |
| 15 | **SL-GLIBC-PRIVATE-CONSUMERS**: patch or drop libnss-db, unscd, wcc, zzuf | S | 11 | package list in the Aqours archive |
| 16 | **LIBM-C23-PI-DOUBLE** / **LIBM-C23-PI** / **LIBM-C23-P1** (per R3) | M / L / L | R3, a 2.41 reference | libm accuracy sweeps; r-base test suite |
| 17 | **SL-AQOURS-REBUILD** (stage 3) | XL (loop) | 11 | per-package pass/fail table, classified |
| 18 | **SL-DEVTOOLS**: `gencat`, `sotruss`, `sprof`, `memusage`, `mtrace`, `libmemusage.so`, `libpcprofile.so` | M | 11 | differential |
| 19 | **M-0/M-1 libmvec** (ledger) | S/S | 7 | re-scoped: only for foreign binaries once Liella's headers declare no `simd` variants |
