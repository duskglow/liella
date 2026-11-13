# RELEASE-PACKAGING-DEB: reference .deb packages that coexist with glibc

**State:** branch `task/release-packaging-deb` (cut from origin/next `7b1d895b0`), code head `b5b4df832`; this note
and `handoff/reports/RELEASE-PACKAGING-DEB.json` on top. Role mei `~/w/s1`.

## Design (written before the build; pending records `records/pending/RELEASE-PACKAGING-DEB-1.toml` and `-2.toml`)

* **Sysroot** `/usr/lib/liella`, laid out as Debian trixie lays out `/` (merged `/usr`, multiarch): libraries in
  `<sysroot>/usr/lib/x86_64-linux-gnu/`, `<sysroot>/usr/lib64/ld-linux-x86-64.so.2` -> `../lib/x86_64-linux-gnu/...`
  (as libc6 ships `/usr/lib64/ld-linux-x86-64.so.2`, seen in the chroot), `lib` -> `usr/lib`, `lib64` -> `usr/lib64`.
* **Packages** (names not Debian's): `liella6` (as libc6: loader + `libc.so.6`, `libm.so.6`, the placeholder
  `.so`s, `libnsl.so.1`) and `liella6-dev` (as libc6-dev, static archives included as Debian does: `crt1.o`,
  `Scrt1.o`, `rcrt1.o`, `libc.a`, `libm.a`, `libc_nonshared.a`, the empty archives, `libm.so` -> `libm.so.6`, the
  `libc.so` script with trixie's GROUP line resolved in the sysroot, `/usr/bin/liella-gcc`, `liella-g++`, man page).
  Outside the sysroot only the wrapper, its man page and `/usr/share/doc/<pkg>`. No maintainer scripts, triggers,
  conffiles or shlibs.
* **Selecting Liella:** the program interpreter `/usr/lib/liella/lib64/ld-linux-x86-64.so.2`. `liella-gcc`
  compiles with the system compiler and the system's headers (Liella has no header set; `liella6-dev` depends on
  `libc6-dev`), links `-nostartfiles -nodefaultlibs`, `-Wl,--sysroot=/usr/lib/liella`, `--dynamic-linker`, Liella's
  `Scrt1.o`, gcc's crtbeginS/crtendS, `-lc -lgcc -lgcc_eh`; `-static` (crt1.o, crtbeginT.o, libc.a,
  `--eh-frame-hdr`); `-shared`. `liella-g++` links libstdc++ and the unwinder statically. `-static-pie`, `-pg` refused.
* **Loader paths (brief item 3): made configurable at build time.** The prefix build pinned Liella's sonames and
  took the default/trusted directory as `<prefix>/lib`, which cannot name the multiarch directory. New
  `LIELLA_LIBDIR` (compile time, default `lib`: every existing build unchanged) -> `search::own_lib`. The packages
  build with `LIELLA_PREFIX=/usr/lib/liella LIELLA_LIBDIR=usr/lib/x86_64-linux-gnu`; cache path
  `/usr/lib/liella/etc/ld.so.cache` (none shipped). Proven in the chroot: `ld.so --list ./hello` resolves
  `libc.so.6 => /usr/lib/liella/usr/lib/x86_64-linux-gnu/libc.so.6`.
* **Build:** `tools/deb/build.sh` (git tree: exports HEAD; role dir without .git: `LIELLA_DEB_COMMIT` +
  `SOURCE_DATE_EPOCH`), release profile, strip, fixed modes/times, `dpkg-deb --root-owner-group -Zxz`.
  **Version** `0.0~snapshotYYYYMMDD+<12 hex commit>`. `tools/deb/check.sh` (static checks), `tools/deb/proof.sh`
  + `inside.sh` (mmdebstrap `--mode=unshare`, output `/dev/null`, official mirrors only). User doc
  `docs/install-packages.md`.

## Results (jobs on mei)

* `rpd-deb-2` (head 4110d663a; later commits change only the plants file): two release builds in two directories
  -> **byte-identical** .debs; `check.sh` PASS; **trixie proof: pass=15 fail=0 wait=5 known=1**. PASS: install;
  PT_INTERP; `--list` pinning; static hello, threads, C++ exception; system programs (ls, dpkg, perl, gcc-built
  program on /lib64 loader) before/installed/purged; `dpkg --verify libc6 libc6-dev` empty before/installed/purged;
  purge; file list after purge == before; sysroot gone.
* `rpd-plants-1`: 5 plants, **5 CAUGHT** (libc6-path link, wrapper without `--dynamic-linker`, three LIBDIR sites);
  rerun on the final head b5b4df832 as `rpd-plants-2`: **5 CAUGHT**, no WARN. `rpd-gate-2` (direct tier, `--base
  origin/next`): **459 passed, 0 failed**, 5 suites, peak 1.2G, rc 0. `rpd-lint-3`: clippy + static rc 0.
  Superseded: `rpd-gate-1` (failed only on `cargo xtask static`: home path in the plants file, fixed b5b4df832; its
  459 tests passed), `rpd-plants-1` (same plants, older path form), `rpd-deb-1` and `rpd-debq-1` (before the
  keyring and `--eh-frame-hdr` fixes), `rpd-lint-1/2`.

**Smoke programs that wait:** dynamic hello, threads, except, iostream: WAIT for the loader's start-time
`DT_NEEDED` search ("the library search is not available in this build"; handoff/DL-SEARCH-CACHE.md line 33; no
story id found on the board - the coordinator should name it). dlopen: WAIT for `dlsym` (DL-16b) and `dlclose`
(DL-17) in libc.so.6. Static iostream: KNOWN Liella defect, see below. The conformance dynamic programs were not run
in the chroot (they need the by-path libc of conformance/run.sh and cargo); the snapshot rerun on its tag should run
them once the start-time search lands.

**lintian** (trixie, `--pedantic`, run in the chroot): `bogus-mail-host` / `-in-debian-changelog` (E): placeholder
Maintainer `liella-packaging@invalid` (no project address exists; `DEB_MAINTAINER` overrides; RULING NEEDED);
`no-code-sections` (E) on the empty `libpthread.a`, `libdl.a`, `librt.a`, `libutil.a`, `libanl.a`, `libresolv.a`:
intended, libc6-dev ships the same empty archives so `-lpthread` etc. link; `shared-library-is-executable` (E) on
`ld-linux-x86-64.so.2`: intended, the loader is a program (`ld.so --list`), as libc6's is 0755;
`repeated-path-segment` (P): inherent in a sysroot mirroring `/usr/lib` under `/usr/lib/liella`;
`copyright-without-copyright-notice` (W): no year/holder notice exists in the repository to copy;
`shared-library-lacks-prerequisites`, `undeclared-elf-prerequisites` (W): libc.so.6 has no DT_NEEDED and the
placeholders need the same package's libc.so.6, which lintian cannot map to a declared dependency.
`static-library-has-unneeded-sections .comment` fixed (4110d663a).

## RULING NEEDED

1. **Devuan excalibur proof:** no authenticated Devuan keyring on mei (not in Ubuntu's archive, not on
   deb.debian.org; deb.devuan.org serves plain HTTP only). Owner: obtain `devuan-keyring` and check its fingerprint
   against devuan.org, e.g. `sudo dpkg -i devuan-keyring_<ver>_all.deb` (or put the key file in
   `~claude/work/RELEASE-PACKAGING-DEB/`), then `KEYRING=/usr/share/keyrings/devuan-archive-keyring.gpg
   tools/deb/proof.sh target/deb excalibur`. Optional: `sudo apt-get install mmdebstrap lintian
   debian-archive-keyring` on mei (this run used mmdebstrap 1.5.7 and the Debian keyring unpacked from
   deb.debian.org into `~/work/RELEASE-PACKAGING-DEB/tools/root`, nothing installed as root).
2. **Maintainer address** for the packages (recommend a project address; today a placeholder).
3. **Pending record -2** (loader trusted/pinned directory from `LIELLA_LIBDIR`, amends D-27.16): a trust-boundary
   detail; recommend accept (compile-time only, default unchanged).
4. **`newlocale(1<<LC_ALL, "C", 0)`**: the reference returns a locale (probe: errno 0), Liella returns NULL/EINVAL.
   libstdc++ imports `__newlocale`; every C++ program using iostream aborts at start on Liella
   ("locale::facet::_S_create_c_locale name not valid"). Spec §7.3 only says "bad mask -> EINVAL": a spec gap.
   Recommend a story: probe what the reference does with that bit, then match it (crates/boundary/src/locale.rs
   `categories`).

## Notes, not to redo

* The loader on next starts no dynamic program built normally; do not chase that here.
* Rule slips this run (my own): one `sed -i` on a scratch job script and one heredoc (python) to edit the plants file.
* **Next step:** after the rulings, the snapshot release runs `tools/deb/build.sh`, `check.sh`, `proof.sh` for both
  suites on its tag. Plan rows: plan/ph14/design.md §4, D-27.16, R14-3.
