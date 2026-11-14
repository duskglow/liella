# RELEASE-PACKAGING-DEB: reference .deb packages that coexist with glibc

**State:** branch `task/release-packaging-deb-2` (on run 1's `task/release-packaging-deb`, cut from origin/next
`7b1d895b0`), code head `06e2a025d`; this note and `handoff/reports/RELEASE-PACKAGING-DEB-2.json` on top (no code).
Run 2 role: shiki `~/w/sh2`. Run 1 (role mei `~/w/s1`) is accepted; run 2 applied rulings #5408 and #5409.

## Design (pending records `records/pending/RELEASE-PACKAGING-DEB-1.toml` and `-2.toml`)

* **Sysroot** `/usr/lib/liella`, laid out as Debian trixie lays out `/` (merged `/usr`, multiarch): libraries in
  `<sysroot>/usr/lib/x86_64-linux-gnu/`, `<sysroot>/usr/lib64/ld-linux-x86-64.so.2` -> the multiarch copy,
  `lib` -> `usr/lib`, `lib64` -> `usr/lib64`.
* **Packages** `liella6` (as libc6) and `liella6-dev` (as libc6-dev, static archives included, `liella-gcc`/`liella-g++`).
  Outside the sysroot only the wrapper, its man page and `/usr/share/doc/<pkg>`. No maintainer scripts.
  **Maintainer** `Liella Maintainers <liella@koiren.ai>` (owner, #5409); `DEB_MAINTAINER` overrides.
* **Selecting Liella:** program interpreter `/usr/lib/liella/lib64/ld-linux-x86-64.so.2` (the wrapper links with
  `--dynamic-linker`, Liella's start files, `--sysroot`).
* **Loader paths:** compile-time `LIELLA_LIBDIR` (default `lib`; record -2, accepted #5410); the packages build with
  `LIELLA_PREFIX=/usr/lib/liella LIELLA_LIBDIR=usr/lib/x86_64-linux-gnu`.
* **Build** `tools/deb/build.sh` (git tree: `git archive` HEAD; role dir without .git: `LIELLA_DEB_COMMIT` +
  `SOURCE_DATE_EPOCH`), version `0.0~snapshotYYYYMMDD+<12 hex>`; `check.sh`; `proof.sh` + `inside.sh`.
  Run 2: the role-dir copy leaves out every cargo build directory (`--exclude-caches-all`: `fuzz/target` was being
  written by the gate while copied, "file changed as we read it") and both copies go through a file, so a failed
  copy stops the build instead of packaging part of the tree.
* **Mirror:** `proof.sh` takes `MIRROR` (default the official mirror); trust is the keyring (signed index).

## Results run 2 (jobs on shiki, code head 06e2a025d)

* `rpd2-deb-2`: two release builds in two directories -> **byte-identical** .debs (`liella6`, `liella6-dev`
  `0.0~snapshot20261114+06e2a025d4f0`), Maintainer as above, `check.sh` PASS;
  **trixie** (deb.debian.org, Debian keyring 2025.1 unpacked as in run 1) **pass=15 fail=0 wait=5 known=1**;
  **excalibur** (owner's LAN mirror `http://miko.mgmt.internal/devuan`, `KEYRING=~/keyrings/devuan-archive-keyring.gpg`;
  gpgv: InRelease good signature by 9F8D6C74DE661075FD171BE3B3982868D104092C, Devuan 6, libc6 2.41-12+deb13u4)
  **pass=15 fail=0 wait=5 known=1**: same PASS set as run 1's trixie proof (install, PT_INTERP, `--list` pinning,
  static hello/threads/C++ exception, system programs and `dpkg --verify libc6 libc6-dev` before/installed/purged,
  purge leaves the file list as before, sysroot gone). mmdebstrap 1.5.7 from `~/work/RELEASE-PACKAGING-DEB-2/tools/root`
  (the same .debs run 1 used, sha256 checked); logs `~/work/RELEASE-PACKAGING-DEB-2/logs/proof-<suite>.log`.
* `rpd2-gate-2` (direct, `--base origin/next`): **459 passed, 0 failed**, peak 1022M. `rpd2-lint-1`: cargo clippy
  `--all-features -D warnings`, `cargo xtask clippy`, `cargo xtask static` rc 0. `rpd2-plants-1`: **5 CAUGHT**, no WARN.
* Superseded: `rpd2-lint-0` (step 0 on the inherited head, green), `rpd2-gate-1` (green, head 28c8781fc before the
  build.sh copy fix), `rpd2-deb-1` (stopped by me: same head, the copy problem showed there).

**lintian** (both suites, `--pedantic`): `bogus-mail-host` gone. Left, each intended (run 1's reasons):
`no-code-sections` (E, the six empty archives libc6-dev also ships), `shared-library-is-executable` (E, the loader is
a program), `repeated-path-segment` (P, the sysroot mirrors `/usr/lib`), `copyright-without-copyright-notice` (W, no
notice in the repository), `shared-library-lacks-prerequisites` / `undeclared-elf-prerequisites` (W, own libc.so.6).

**Smoke programs that wait (both suites):** dynamic hello, threads, except, iostream: the loader's start-time
`DT_NEEDED` search ("the library search is not available in this build"). dlopen: `dlsym`/`dlclose` in libc.so.6
(DL-16b, DL-17). Static iostream: KNOWN, `newlocale(1<<LC_ALL, "C", 0)` (story NEWLOCALE-LC-ALL-BIT, #5411).

## Open / next

* No ruling open. Next: the snapshot release runs `build.sh`, `check.sh`, `proof.sh` for both suites on its tag
  (trixie from deb.debian.org until the owner's Debian mirror is up, then `MIRROR=`), and the conformance dynamic
  programs once the start-time search lands. Plan rows: plan/ph14/design.md §4, D-27.16, R14-3.
* Plants file `handoff/RELEASE-PACKAGING-DEB-plants.txt` names the role's `target/deb-work1` products: change the
  role path for a new host and run a deb build first.
* Not to redo: the loader on next starts no dynamic program built normally; do not chase that here.
* Rule slips run 2 (mine): one `sed -i` on the plants file comment and one empty heredoc in a Bash call.
