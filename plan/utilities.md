# The libc utilities: how Liella provides them (LIELLA-UTILITIES-DESIGN)

**Status: Proposed. The owner reviews this design before any implementation story starts.**

Builds on the owner's rulings of 2026-11-13/14: R1 (in Aqours, Liella's packages take the names `libc6`,
`libc-bin`, `libc6-dev`, `libc-dev-bin`, `locales`, `libc-l10n`), R5 (Devuan's locale data compiled by Liella's own
localedef now, a CLDR-derived set later), R6 (ldconfig, ldd, getent, getconf, iconv, locale and localedef come into
Liella's spec; ldconfig first), R7 (zic, zdump and tzselect reimplemented clean-room), R8 (no nscd), #941 (the loader
reads only the new cache format), and the owner's principle **not how glibc does it**: what has to stay compatible is
what callers depend on (command names, options, output, exit codes, file formats), never the reference's internal
structure, and every shape below is weighed against the alternatives.

**Evidence.** Black-box probes of the reference system (Ubuntu 24.04.5; `libc-bin`, `libc-dev-bin` and `locales`
2.39-0ubuntu8.9; `tzdata` 2026e; `dpkg` 1.22.6): `tools/probe/util_surface.sh` and `tools/probe/util_callers.sh` run
the programs, read package metadata and search non-libc scripts for invocations. They read no libc or tzcode source
and no libc script (`ldd`, `tzselect`, the `ldconfig` wrapper, `locale-gen`, `update-locale` and `validlocale` are
excluded from every content search). Also: man pages (ldconfig(8), ldd(1), getent(1), getconf(1), iconv(1), locale(1),
localedef(1), zic(8), zdump(8), tzselect(1), tzfile(5), locale-gen(8), ld.so(8)), POSIX, RFC 8536, and the spec's
Appendix D (D.1 locales, D.2 catalogs, D.3 the loader cache). Numbers below are from those probes unless marked.

## 1. Summary

* **One library per domain, holding both sides of each file format.** Today the readers live in Liella (TZif in
  `liella-core::time::tzif`, locales in `liella-core::locale`, charmaps in `liella-core::charmap`, the loader cache in
  `liella-ld::cache`); the utilities are the writers. Putting each writer in the crate of its reader gives one
  implementation per format, round-trip tests by construction, and one fuzz target per parser.
* **Commands are thin front ends, grouped into one executable per domain** (`liella-tz` answers as zic, zdump and
  tzselect; `liella-locale` as locale, localedef, locale-gen, update-locale and validlocale), chosen by `argv[0]`.
  **ldconfig stands alone: a freestanding static binary** with no C library dependency, because dpkg refuses to run
  without it and it runs as root on every package operation.
* **Public interfaces:** the command lines and file formats only, on day one. Proposed for the owner's yes: a public
  Rust `tz` API (read, list transitions, compile) in `liella-native`'s clean API once zic and zdump are proven. No new
  C ABI until a C consumer asks.
* **Order:** spec part and transcript harness, then ldconfig with the cache writer; then ldd, getent and getconf; then
  iconv and locale; then the tz tools; then localedef with the `locales` commands; gencat and pldd last.

## 2. The compatibility surface

### 2.1 What the reference packages ship

| Path | Package | Kind (reference) | Liella |
|---|---|---|---|
| `/usr/sbin/ldconfig` (+ `ldconfig.real`) | libc-bin | wrapper script + static-pie ELF | one binary at `/usr/sbin/ldconfig` |
| `/usr/bin/ldd` | libc-bin | bash script | front end over Liella's loader |
| `/usr/bin/getent`, `getconf`, `iconv`, `locale`, `localedef`, `zdump`, `pldd` | libc-bin | dynamic ELF | front ends (§4) |
| `/usr/sbin/zic`, `/usr/sbin/iconvconfig` | libc-bin | dynamic ELF | zic in `liella-tz`; iconvconfig: Q4 |
| `/usr/bin/tzselect` | libc-bin | bash script | command of `liella-tz` |
| `/usr/bin/ld.so` | libc-bin | link to `../lib64/ld-linux-x86-64.so.2` | the same link, to Liella's loader |
| `/etc/ld.so.conf` (`include /etc/ld.so.conf.d/*.conf`), `/etc/ld.so.conf.d/libc.conf` (`/usr/local/lib`) | libc-bin | config | the same |
| `/etc/ld.so.conf.d/x86_64-linux-gnu.conf` | libc6 | config | the same |
| `/etc/gai.conf`, `/usr/share/libc-bin/nsswitch.conf` | libc-bin | config / template | Liella's own defaults |
| `/etc/bindresvport.blacklist` | libc-bin | config | Q4 (Sun RPC is not implemented) |
| `/usr/lib/locale/C.utf8/` | libc-bin | compiled locale | built into Liella; `locale -a` still lists it |
| `/var/cache/ldconfig/` | libc-bin | directory | Liella's own auxiliary cache, if UTIL-3 measures a gain |
| `/usr/bin/gencat` | libc-dev-bin | dynamic ELF | UTIL-14 |
| `/usr/sbin/locale-gen`, `update-locale`, `validlocale`, `/etc/locale.alias` | locales | scripts, config | commands of `liella-locale` (Q8) |
| `/usr/bin/sprof`, `sotruss`, `mtrace` | libc-devtools | ELF, scripts | Q4 |
| `nscd` | (not installed) | | dropped (R8) |

`tzdata` ships data only: TZif version 2, "fat" files (Europe/London 3 664 bytes), `tzdata.zi` (`# version 2026e`,
447 zones, 151 links), `zone.tab`, `zone1970.tab`, `zonenow.tab`, `iso3166.tab`, `leapseconds`, `leap-seconds.list`;
no `posix/` or `right/`. `/etc/localtime` is a symlink into `/usr/share/zoneinfo`; `/etc/timezone` holds the name.

### 2.2 Who the callers are

* **dpkg** refuses to unpack or configure anything when `ldconfig` is not in `PATH`: `dpkg: warning: 'ldconfig' not
  found in PATH or not executable` ... `dpkg: error: 2 expected programs not found in PATH or not executable`, exit 2
  (the other is `start-stop-daemon`). An Aqours root needs an executable `ldconfig` before its first package operation.
* **dpkg triggers:** 398 of the 440 `.triggers` files on the reference say `activate-noawait ldconfig` (404 packages
  ship `.shlibs`); `libc-bin` is the one package interested in the `ldconfig` trigger.
* **Maintainer scripts** outside the libc packages: `getent` in 18 (`getent passwd` 13, `getent group` 13, `getent
  shadow` 1), bare `ldconfig` in 3, `iconv` in 1; none use ldd, getconf, localedef, locale-gen, update-locale, zic,
  zdump, tzselect or iconvconfig. (A build server's package set; the mirror scan of LIELLA-SYSTEM-LIBC-PLAN recounts
  over the whole archive.)
* **Boot:** initramfs-tools (`hook-functions`, `mkinitramfs`, hooks `cryptroot` and `dhcpcd`) parses `ldd` output:
  lines starting with a TAB, the path after `=>`, the word before ` (`, and the multiarch directory taken from the path
  of `libc.so.N` in `ldd /bin/sh`. Wrong `ldd` output makes an unbootable initramfs.
* **Language runtimes and tools:** CPython's `ctypes.util.find_library` runs `/sbin/ldconfig -p` and matches
  `\s+(lib<name>\.[^\s]+)\s+\(libc6,x86-64`; `sos` and an initramfs hook also read `ldconfig -p`.
* **Scripts** (search of the reference's non-libc scripts): `locale charmap` (6 files), `locale -a` (3), `getent
  passwd` (4), `ldconfig -p`/`-r`/`-v`, `getconf _NPROCESSORS_ONLN`, `getconf PAGESIZE`.
* **Builds** (Aqours rebuilds from source, route A): `getconf` for job counts, `gencat` for X/Open catalogs, `zic` for
  the `tzdata` package itself, `localedef` for packages that build locales.
* **The loader:** Liella's ld.so reads `/etc/ld.so.cache` in the new format only (D-27.16, #941). Other programs may
  read the binary file directly; its layout (D.3) is a de facto interface.

### 2.3 Each utility

**ldconfig.** `/usr/sbin/ldconfig` (CPython names `/sbin/ldconfig`, which merged `/usr` makes the same file).
Options (`--help`): `-c/--format=new|old|compat`, `-C CACHE`, `-f CONF`, `-i/--ignore-aux-cache`, `-l`, `-n`, `-N`,
`-p/--print-cache`, `-r ROOT`, `-v/--verbose`, `-X`, `-?/--help`, `--usage`, `-V/--version`.
* Reads `/etc/ld.so.conf` (`include` lines with globs, `#` comments), then the built-in trusted directories (seen in a
  private root: `/lib/x86_64-linux-gnu`, `/usr/lib/x86_64-linux-gnu`, `/lib`, `/usr/lib`). A directory reached twice
  (merged-`/usr` links) is scanned once, with a message; the loader's own file is skipped, with a message.
* Writes the cache in the D.3.1 new format with the D.3.2 extensions (tag 0 generator text, tag 1 hwcaps names),
  through a temporary file (`Can't create temporary cache file /etc/ld.so.cache~: Permission denied`, exit 1, as an
  ordinary user), and soname links in the scanned directories.
* Link rule (probed with `-n DIR`): the link `<soname>` points to the file with the highest version under the
  numeric comparison of D.3.3 (`libfoo.so.1` -> `libfoo.so.1.10`, not `1.9` or `1.2`); a file without `DT_SONAME` is
  keyed by its file name and gets no link; development links (`libbar.so`) are left alone.
* `-p`: `567 libs found in cache `/etc/ld.so.cache'`, then `\t<key> (libc6,x86-64) => <path>` per entry, then
  `Cache generated by: <tag 0 text>`. Missing cache: `Can't open cache file <path>` + strerror, exit 1.
* `-r ROOT`: configuration, directories and cache are inside ROOT; printed paths are root-relative.

**ldd.** `/usr/bin/ldd [OPTION]... FILE...`: `--help`, `--version`, `-d/--data-relocs`, `-r/--function-relocs`,
`-u/--unused`, `-v/--verbose`.
* Lines `\tlinux-vdso.so.1 (0x…)`, `\t<name> => <path> (0x<16 hex digits>)`, `\t/lib64/ld-linux-x86-64.so.2 (0x…)`;
  with several files a `<file>:` line before each list; `-v` appends `\tVersion information:` and per object
  `\t\t<needed> (<version>) => <path>`.
* Not ELF: `\tnot a dynamic executable`, exit 1. Static or static-pie: `\tstatically linked`, exit 0. Missing file:
  `ldd: <file>: No such file or directory`, exit 1. No operand: `ldd: missing file arguments` and the `Try` line, exit 1.
* `LD_TRACE_LOADED_OBJECTS=1 PROGRAM` prints the same list (ld.so(8)). Liella's loader already implements `--list` and
  the variable with these line formats (D-27.16).

**getent.** `/usr/bin/getent [OPTION...] database [key ...]`: `-A/--no-addrconfig`, `-i/--no-idn`,
`-s/--service=CONFIG`, `-?/--help`, `--usage`, `-V/--version`. Databases: `ahosts ahostsv4 ahostsv6 aliases ethers
group gshadow hosts initgroups netgroup networks passwd protocols rpc services shadow`. Output is the database's file
line (`root:x:0:0:root:/root:/bin/bash`), `hosts` as `<address padded> <names>`, `ahosts` as `<address> STREAM
<name>` then `DGRAM` and `RAW` lines, `services` as `<name padded> <port>/<proto>`, `initgroups` as `<user padded>
<gids>`. Exit 0 found; 1 usage or unknown database (`Unknown database: <db>` and the `Try` line); 2 key not found (no
output); 3 enumeration not supported (getent(1)). The answers are the C library's (§16); getent adds only formatting.

**getconf.** `getconf [-v spec] NAME [PATH]`, `getconf -a [PATH]`, `--help`, `--version`. `-a` prints `NAME`, padding,
value per line (320 lines). Names include the POSIX set and `GNU_LIBC_VERSION` (`glibc 2.39`), `GNU_LIBPTHREAD_VERSION`
(`NPTL 2.39`), `PAGESIZE`, `PAGE_SIZE`, `_NPROCESSORS_ONLN`, `LONG_BIT`, `PATH` (`/bin:/usr/bin`), `LFS_CFLAGS`
(empty), `POSIX_V7_LP64_OFF64_CFLAGS` (`-m64`). Unknown name: ``getconf: Unrecognized variable `NAME'``, exit 2; a
path variable without a path: usage, exit 2; `-v` with an unknown specification was accepted (exit 0). Values come
from `sysconf`, `confstr`, `pathconf` (§24); getconf adds the name table.

**iconv.** `iconv [OPTION...] [FILE...]`: `-f/--from-code`, `-t/--to-code` (`//TRANSLIT`, `//IGNORE`),
`-l/--list`, `-c`, `-o/--output`, `-s/--silent`, `--verbose`, `--help`, `--usage`, `-V`. Errors: `iconv: illegal input
sequence at position N` (exit 1, output up to that point written); ``iconv: conversion from `X' is not supported``
and the `Try` line (exit 1); ``iconv: cannot open input file `F': <strerror>`` (exit 1). `-c` dropped an
unconvertible character with exit 0. `-l` lists 1 180 names (`437//` style) from 253 gconv modules. Liella has no
gconv modules (D-6.8): its charsets are the built-ins and the 233 charmaps in `/usr/share/i18n/charmaps`, so its `-l`
lists exactly the names it can convert.

**locale.** `locale [OPTION...] NAME...` or `[-a|-m]`: `-a`, `-m`, `-c`, `-k`, `-v`, `--help`, `--usage`, `-V`.
Without operands: `LANG=`, `LANGUAGE=`, the twelve `LC_*` (quoted when derived), `LC_ALL=`. `-a`: `C`, `C.utf8`,
`POSIX` and installed names with normalised codesets (`en_US.utf8`); `-a -v`: each locale's source (`archive:
/usr/lib/locale/locale-archive` or `directory: …`) and its `LC_IDENTIFICATION` fields. `-m`: charmap names. A keyword
or category operand prints values; `-k` as `keyword=value` (strings quoted, numbers bare, including names such as
`numeric-decimal-point-wc`); `-c` adds the category name. Unknown operand: `locale: unknown name "<n>"`, exit 1. A
locale variable naming a missing locale: `locale: Cannot set LC_… to default locale: No such file or directory` on
stderr, exit 0.

**localedef.** `localedef [OPTION...] NAME`, `--add-to-archive|--delete-from-archive FILE...`, `--list-archive [FILE]`.
Options: `-f/--charmap`, `-i/--inputfile`, `-u/--repertoire-map`, `-c/--force`, `--no-hard-links`, `--no-warnings=`,
`--warnings=` (`ascii`, `intcurrsym`), `--posix`, `--prefix=`, `--quiet`, `-v`, `-A/--alias-file`, `--big-endian`,
`--little-endian`, `--no-archive`, `--replace`, `--help`, `--usage`, `-V`. Paths (`--help`): charmaps
`/usr/share/i18n/charmaps`, repertoire maps `/usr/share/i18n/repertoiremaps`, locale path `/usr/lib/locale:/usr/share/i18n`.
A NAME containing `/` receives the directory form (twelve files; `en_US` with UTF-8: `LC_COLLATE` 2 586 930 bytes,
`LC_CTYPE` 360 460); otherwise the archive (D.1.6) unless `--no-archive`. Errors: `[error] …` lines and exit 4 when no
output was produced (missing source, missing charmap). Source data: 233 charmaps, the locale sources and
`/usr/share/i18n/SUPPORTED` (package `locales`).

**zic, zdump, tzselect.** zic: `--version`, `--help`, `-v`, `-b slim|fat`, `-d DIR`, `-l LOCALTIME`, `-L LEAPSECONDS`,
`-p POSIXRULES`, `-r '[@lo][/@hi]'`, `-t LINK`, files. zdump: `-c [L,]U`, `-t [L,]U`, `-i`, `-v`, `-V`, `--help`,
`--version`. Formats: plain `Europe/London  Sat Nov 14 02:52:48 2026 GMT`; `-v` per transition `<zone>  <UT> UT =
<local> <abbr> isdst=<0|1> gmtoff=<s>`, and `<zone>  -9223372036854775808 = NULL` style lines at the ends of the
range; `-i` (marked experimental in its own help) `TZ="<zone>"` then tab-separated date, time, offset, abbreviation
and DST flag. tzselect: `--help`, `--version`, `-c COORD` (ISO 6709), `-n LIMIT` (default 10); interactive; prints the
chosen value. The reference zic, given the shipped `tzdata.zi`, reproduces all 447 shipped zone files byte for byte
(default and `-b fat`).

## 3. Architectures

* **(a) Separate programs**, each with its own copy of its logic: the reference's shape, the baseline to beat.
* **(b) A library per domain with thin front ends:** the logic, and both sides of each format, in one Rust crate per
  domain; each command a small program over it.
* **(c) One multi-call binary** for every command, chosen by `argv[0]`.
* **(d) Domain executables:** (b)'s libraries, and one executable per domain whose commands are names of it; a domain
  with one command is a plain program; ldconfig freestanding and static.
* **(e) Executable libraries** (the owner's "libtz", taken literally): a shared object per domain with a C ABI
  (`libliella-tz.so.1`) that, when executed, runs as zic, zdump or tzselect by `argv[0]` (the loader and the C library
  are already executable shared objects).

| | (a) separate | (b) library + front ends | (c) one multi-call | (d) domain executables | (e) executable library |
|---|---|---|---|---|---|
| Code sharing | none between a tool and the libc: two implementations of each format | one crate per domain: writer beside reader | as (b), plus the front ends | as (b), plus one copy per domain | as (d) |
| Size | each program carries its common code | same as (a) | smallest | one copy per domain | as (d), plus a dynamic symbol table |
| Start-up | per program | per program | every start relocates and touches the code of every command | per domain | per domain, plus symbol binding |
| Debian packaging | maps onto the split | maps onto the split | one package owns all; others ship links | maps; `locales` commands are links into the Essential `libc-bin` | adds a shared-library package per domain (soname, symbols file) |
| Privilege | ldconfig on its own | same | ldconfig shares an executable and start-up path with everything | ldconfig on its own, minimal | as (d), and a root-run shared object on the search path |
| Testability | only through processes | library tests and fuzzing without processes; transcripts for front ends | as (b), one binary | as (b), one binary per domain | as (d) |
| Attack surface | per program | per program | every process maps every parser | per domain | per domain, plus a C ABI any program can call |
| Use by other programs | spawn and parse text | Rust links the crate; C spawns | spawn | Rust links the crate; C spawns | C calls the ABI |
| Compat promise | commands and formats | the same | the same | the same | the same, plus a versioned C ABI per domain |

**Choice: (d).** The structural gain is (b)'s: one implementation of each format, reader and writer tested against
each other. On top of that, one executable per domain keeps start-up and attack surface per domain (unlike (c)),
ships one artifact per domain (the owner's libtz framing without an ABI promise), and leaves ldconfig minimal. (e)
remains the upgrade path: if a C consumer appears for a domain, its crate is also built as a C ABI library with no
change to the commands. (a) is rejected because it gives up the shared reader and writer. UTIL-SHAPE measures (d)
against (b) on start-up instructions, page faults and size; if a domain executable costs more than separate programs,
that domain falls back to (b), which changes the build only.

**Public interfaces.**
* Public already, with the compatibility promise of §2: the commands and the formats (TZif per RFC 8536, the zic
  input language per zic(8), the D.3 cache, the D.1 locale directory and archive, POSIX charmaps and locale sources,
  `/etc/ld.so.conf`, `/etc/locale.gen`).
* **Proposed (owner's yes):** a Rust `tz` module in `liella-native`'s clean API (D-X.10: stateless computation compiled
  into the consumer, semver-versioned): read a TZif file, list a zone's transitions in a range (zdump as a function),
  compile zic source to TZif bytes, read the zone tables tzselect uses. Installers, date and calendar tools and language
  runtimes get RFC 8536 handling without spawning zic or writing their own parser. After UTIL-10 proves it.
* Deferred: a `locale` listing/compiling API (no POSIX interface lists locales; programs parse `locale -a`), after
  UTIL-12, since name and alias resolution are still settling (D-7.7).
* Private: the cache writer and reader (CPython parses `ldconfig -p`; the loader is the consumer), the iconv charset
  list, getconf's name table, getent (the NSS functions are the API).
* No new C ABI: D-X.10 keeps the native C ABI for stateful services; stateless C entry points would be a new category.

## 4. Recommendation per domain, and the order

**Loader cache (ldconfig).** `liella-ld` gains `cache::write` beside the reader (shared constants and the D.3.3
comparison `cache::compare`, so writer order and reader search agree by construction), a `conf` parser (`include` with
globs, comments; `/etc/ld.so.conf.d/*.conf` read in sorted order), a directory scan (sorted listings, the ELF reader the
loader uses, class and machine checks, `glibc-hwcaps/<level>` subdirectories) and the link rule. ldconfig is
freestanding (no C library, like ld.so) and static, so it runs in a root with no libc installed and while `libc6` is
being replaced.
* Format: exactly the D.3.1 new format with tag 0 and tag 1 extensions: what Liella's loader reads, what direct
  readers of the file expect, and the same on Debian coexistence systems. `-c old|compat`: Q5.
* Gains, described as Liella behaviour: **deterministic output** (entries sorted by the D.3.3 comparison, ties broken
  by configuration order and then path bytes, directory listings sorted: identical input trees give byte-identical
  caches on any file system); **atomic replacement** (temporary file in the same directory, `fsync`, `rename`); every
  ELF file treated as untrusted input (bounds-checked, never mapped executable); **`-r` without privileges** through
  `openat2(RESOLVE_IN_ROOT)` instead of `chroot` (ordinary users and tests can build roots; `chroot` only as the
  fallback where `openat2` is missing).
* dpkg: Liella's `libc-bin` declares interest in the `ldconfig` trigger and runs `ldconfig` once per trigger cycle.
  An auxiliary cache (Liella's own file under `/var/cache/ldconfig/`, never the reference's) only if UTIL-3's counters
  show a gain on a full library tree.

**ldd.** A front end that runs Liella's loader in `--list` mode on each operand (the loader's own search, cache and
hwcaps choice, D-27.16), so ldd and the loader cannot disagree. It never runs the operand's code; for a file whose
`PT_INTERP` is not Liella's loader it lists by Liella's rules and says so on stderr (Q6). `-v` reads the version needs
itself. `-d`, `-r` and `-u` need relocation processing (DL-10); until then they are refused with a message.

**Name service (getent).** A thin dynamic front end over the public NSS functions, so it shows exactly what every
program gets (modules, `nsswitch.conf`, the integrated cache D-16.10). `-s` uses `__nss_configure_lookup`.

**getconf.** A thin front end over `sysconf`, `confstr` and `pathconf`; its name table is generated from the spec's
tables, so a name added to the library is added to getconf in the same change.

**iconv.** A thin front end over `iconv(3)` (the command is a client of the API like any program); `-l` from the
iconv crate (built-ins, charmap names and their `% alias` lines). Liella's charmaps cost a decompress and parse per
process (D-6.8); a compiled charmap table installed by the package is a possible later performance story, only with
counters showing the gain. `iconvconfig`: Q4.

**Locale (locale, localedef, and the `locales` commands).** A `liella-locale` crate: the locale source parser (POSIX
localedef syntax and the extensions Devuan's sources use), the charmap parser shared with iconv (`core::charmap`), a
writer for the D.1 category files and the D.1.6 archive (add, delete, list, replace), and locale listing. Output stays
in the D.1 format: Liella's reader reads it, coexistence systems need it, and a Liella-native format for the CLDR set
(R5) can be proposed later with measurements. `locale-gen`, `update-locale` and `validlocale` (interfaces from their
man pages, `/etc/locale.gen`, `/etc/default/locale`, `SUPPORTED`) compile in-process: one parse of each charmap and of
shared sources such as the common collation table for all requested locales, in parallel; counters per locale against
one `localedef` process per locale decide the claim.

**Time zones (zic, zdump, tzselect).** A `liella-tz` crate: the zic input parser (zic(8): `Rule`, `Zone`, `Link`,
`Leap`, `Expires`), the rule engine over the full 64-bit range, the TZif writer (RFC 8536 versions 2-4, `-b slim|fat`,
`-r` truncation), the footer generator with a check that the footer reproduces the last transitions (RFC 8536 §3.3),
leap seconds, the zdump renderer (`-v`, `-V`, `-i`, `-c`, `-t`), and the tzselect tables (`zone1970.tab`,
`zonenow.tab`, `iso3166.tab`, ISO 6709 distances for `-c`). The TZif reader and POSIX rule parser move here from
`liella-core::time` (or are shared from there), so the C library and the tools read with one implementation. Default
`-b fat`: the observed reference default and the form the shipped files have (Q9 is local).

**Order.**
1. **ldconfig** (UTIL-2, UTIL-3): dpkg will not run without it; freestanding, so it is in the very first Aqours root.
2. **ldd, getent, getconf** (UTIL-4 to 6): initramfs generation, maintainer scripts, builds. The dynamic front ends
   run once DL-10 lets the loader start programs with dependencies (a prerequisite of Aqours as a whole).
3. **iconv, locale** (UTIL-7, 8).
4. **zic, zdump, tzselect** (UTIL-9 to 11). `tzdata` is architecture-independent data, so Aqours can take Devuan's
   package unchanged until it rebuilds `tzdata` itself (then zic is a build tool).
5. **localedef and the `locales` commands** (UTIL-12, 13): R5; not bootstrap-critical (C.UTF-8 is built in).
6. **gencat, pldd** (UTIL-14, 15); the public `tz` API (UTIL-16) after the owner's yes; the CLDR set (R5) later.

## 5. Proof plan

Common to all: a transcript harness (UTIL-1) runs the reference command and Liella's with the same argv, environment,
stdin and file tree, inside an unprivileged user and mount namespace when a private root is needed, and compares
stdout, stderr and exit status; a skipped case prints why. Each domain library gets property tests, a `cargo fuzz`
target per parser, and plants through `cargo xtask plants`. Identity strings (Q3) are masked in comparisons.

| Utility | Oracle | Corpus | Comparison |
|---|---|---|---|
| cache writer | Liella's reader; the reference `ldconfig -p`; the reference loader | generated caches: sonames with digit runs, hwcaps levels, duplicates, empty, maximal | round trip writer -> reader (property test, entries and order); the reference `ldconfig -p -C` lists Liella-written caches as Liella's `-p` does; the reference loader resolves through a Liella cache bind-mounted over `/etc/ld.so.cache` (the D.3 method); fuzz the reader with mutated writer output |
| ldconfig | the reference ldconfig in the same private roots | generated roots: versions `1.2/1.9/1.10`, no soname, dev links, `include` globs, merged-`/usr` duplicates, hwcaps subdirectories, foreign-class and truncated ELF files, the reference system's own library tree | decoded cache entries equal (byte equality reported, not required), link trees (`find -type l`, sorted) equal, `-p`/`-v`/`-n`/`-X`/`-N`/`-C`/`-f`/`-r` transcripts; determinism: the same tree built in two orders and on two file systems gives identical bytes; counters (instructions, syscalls) for a full run against the reference |
| ldd | the reference ldd | dynamic, static, static-pie, scripts, missing dependencies, several operands, `-v` | transcripts with addresses masked; initramfs-tools' copy_exec parsing of both outputs gives the same file list |
| getent | the reference getent | private `/etc` (passwd, group, shadow, gshadow, hosts, services, protocols, networks, ethers, aliases, netgroup, rpc) in a mount namespace | transcripts per database, key forms, `-s`, enumeration, exit codes 0/1/2/3 |
| getconf | the reference getconf | every name of `getconf -a`, each with and without a path | name set equal; values equal on the same host; error transcripts |
| iconv | the reference iconv | built-in charsets and every charmap: valid, invalid, truncated input; `-c`, `//TRANSLIT`, `//IGNORE`; input split at every byte | output bytes, error position and exit status; `-l` checked as "every listed name converts" |
| locale | the reference locale | every keyword and category of every installed locale; bad environments | transcripts |
| zic | Debian's compiled files; the reference zic and zdump | the shipped `tzdata.zi` (2026e: 447 zones, 151 links), later releases, `-b slim`, `-r` ranges, leap seconds | semantic equality per zone: transitions (time, UT offset, DST flag, abbreviation) over the full range, the footer (evaluated per year, 1800-2500), leap records, links; byte equality reported; the reference `zdump -v` over Liella's files equals it over the shipped files; Liella's `localtime` agrees on both; fuzz the parser; differential fuzz: random zic sources through both compilers |
| zdump | the reference zdump | every zone, `-v`, `-V`, `-i`, `-c`, `-t` | exact transcripts |
| tzselect | the reference tzselect | scripted sessions over every continent and country; `-c` for sample coordinates; `-n` | transcripts of the chosen value and prompts |
| localedef | Liella's libc as the fixed reader | every locale in `SUPPORTED` compiled by both localedefs into `LOCPATH` directories | Liella's libc gives identical results on both: every `nl_langinfo` item, `strcoll`/`strxfrm`/`wcscoll` over a corpus, classification and case mapping for every code point, `strftime`, `strfmon`, grouping; archive add/delete/list round trips; error exit codes; fuzz the source parser |
| locales commands | the reference commands | `/etc/locale.gen` variants in a namespace root | resulting locale sets and files; counters per locale |
| gencat | Liella's `catopen` | catalog sources | Liella-written and reference-written catalogs give the same `catgets` results |

## 6. Stories

| ID | Title | Size | Depends on | Proof |
|---|---|---|---|---|
| UTIL-0 | Spec part "Utilities" (a new section with clause ids) from §2, plus a decision series and view for it | M | owner's yes, Q7 | `cargo xtask stubs`/static green; every surface row cites a probe or a man page |
| UTIL-1 | Transcript harness (reference vs Liella, private roots in user and mount namespaces) | M | UTIL-0 | plants on the comparator (a byte, a status, a stream) caught |
| UTIL-SHAPE | Measure (d) against (b): start-up instructions, faults and size for two front ends in both shapes | S | UTIL-1, DL-10 | counters on both shapes; decision record |
| UTIL-2 | Cache writer in `liella-ld` (deterministic, D.3 new format, tags 0 and 1) | M | owner's yes | round trip, reference `-p`, reference loader, fuzz |
| UTIL-3 | ldconfig: conf parser, scan, links, `-r` via `openat2`, atomic replace, freestanding static product, dpkg trigger in the package | M | UTIL-1, UTIL-2 | differential roots, determinism, consumer check, counters |
| UTIL-4 | ldd over the loader's `--list`; `-v`; `-d/-r/-u` after DL-10 | S | UTIL-1, DL-10 | transcripts, initramfs parsing |
| UTIL-5 | getconf with a generated name table | S | UTIL-1 | name set, values, errors |
| UTIL-6 | getent (dynamic) | S | UTIL-1, DL-10 | transcripts in private roots |
| UTIL-7 | iconv command and charset listing | S | UTIL-1 | transcripts, split inputs |
| UTIL-8 | locale command (keyword table, `-a -v`, `-m`) | S | UTIL-1 | transcripts |
| UTIL-9 | `liella-tz`: parser, rule engine, TZif writer, footer check; the reader shared with the libc | M | UTIL-1 | semantic equality over all zones, fuzz, differential fuzz |
| UTIL-10 | zic and zdump commands (the tz domain executable) | S | UTIL-9 | zdump transcripts for every zone, zic option matrix |
| UTIL-11 | tzselect command | S | UTIL-9 | scripted sessions |
| UTIL-12 | `liella-locale`: source parser, D.1 writer, archive writer, listing; localedef command | L | UTIL-1, UTIL-8 | libc-as-reader equality over all `SUPPORTED` locales, fuzz |
| UTIL-13 | locale-gen, update-locale, validlocale (in-process, parallel) | S | UTIL-12, Q8 | results, counters per locale |
| UTIL-14 | gencat (D.2.2 writer) | S | UTIL-1 | `catgets` equality |
| UTIL-15 | pldd | S | UTIL-1 | transcripts on a test process |
| UTIL-16 | Public Rust `tz` API in `liella-native` (docs, semver) | M | UTIL-10, Q2 | API tests, docs |

Packaging (the trigger declaration, conffiles, links of the domain executables into `/usr/bin` and `/usr/sbin`) goes
with the Aqours packaging story that takes over `libc-bin`, `libc-dev-bin` and `locales` (R1); UTIL-3 provides the
trigger handling it needs.

## 7. Questions for the owner

* **Q1 Architecture:** approve (d): a library per domain holding both sides of each format, one executable per domain,
  ldconfig freestanding and static (recommended); UTIL-SHAPE may move a domain to (b).
* **Q2 Public interfaces:** a public Rust `tz` API in `liella-native` after UTIL-10 (recommend yes); a `locale` API
  deferred; no new C ABI until a C consumer asks (recommend).
* **Q3 Identity strings:** `--version` banners, the bug-report line of `--help`, and the `Cache generated by:` text.
  Recommend Liella's own identity (`<tool> (Liella <version>) 2.39`) after the mirror scan counts scripts that match
  the reference's banners (for example to tell C libraries apart from `ldd --version`); `getconf GNU_LIBC_VERSION`
  follows `confstr`, a library decision outside this plan.
* **Q4 Not shipped:** `iconvconfig` (no gconv modules; no caller found), `/etc/bindresvport.blacklist` (Sun RPC not
  implemented), `libc-devtools` (`sprof`, `sotruss`, `mtrace`). Recommend dropping them with differences rows, unless
  the mirror scan finds users; gencat and pldd are kept (UTIL-14, 15).
* **Q5 `ldconfig -c old|compat`:** refuse with a message, consistent with #941 (recommend); differences row, no
  restore flag (nothing in Liella reads those formats).
* **Q6 ldd on a program for another loader:** list by Liella's rules with a note on stderr; never run another
  interpreter (recommend).
* **Q7 Spec home:** a new spec section "Utilities" with its own decision series and view (a records schema change for
  the coordinator); recommend.
* **Q8 `locales` commands:** reimplement locale-gen, update-locale and validlocale from their man pages inside
  `liella-locale` (recommend; R1 gives Liella the `locales` package).

Local details decided in the stories (decision records there): zic's default `-b fat` (Q9), the auxiliary cache, the
`-r` mechanism, getconf's padding, the masking rules of the harness.

## 8. Gaps

* Devuan excalibur's `libc-bin` (glibc 2.41) file list was not probed (this story may not download packages); compare
  when the mirror is served. The caller counts are from one build server; the mirror scan recounts.
* getent's reach depends on which NSS modules Liella loads (§16); getent itself adds nothing.
* The `ldconfig -p` label of hwcaps entries, `localedef`'s exit status 1 (output written with warnings) and the exact
  `-a` padding of getconf are left to the stories' probes.
