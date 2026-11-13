# DL-LIBC-NONSHARED: Liella's libc_nonshared.a and installed libc.so (owner ruling #4807)

**State.** Branch `task/dl-libc-nonshared` (from `origin/next`). The head is in the hand-back and in
handoff/reports/DL-LIBC-NONSHARED.json. Jobs ran on mei, role `~/w/s3`. Decision **D-27.28** (records/decisions/D-27/D-27.28.toml).

## What landed
* `csys/nonshared/{atexit,at_quick_exit,pthread_atfork,stack_chk_fail_local}.c`: one-statement members, hidden, no
  state. Each passes the value of the caller module's `__dso_handle` to `__cxa_atexit@GLIBC_2.2.5`,
  `__cxa_at_quick_exit@GLIBC_2.10` or `__register_atfork@GLIBC_2.3.2`. `__stack_chk_fail_local` calls
  `__stack_chk_fail@GLIBC_2.4`.
* `xtask/src/so.rs` (`build_nonshared`, `NONSHARED_MEMBERS`, `LIBC_SO_SCRIPT`): `cargo xtask so` writes
  `target/liella/libc_nonshared.a` (`ar rcsD`; each member must define exactly its names, all HIDDEN; members are
  compiled by relative name, so the archive holds no build path) and `target/liella/libc.so` =
  `GROUP ( libc.so.6 libc_nonshared.a AS_NEEDED ( ld-linux-x86-64.so.2 ) )` (bare names).
* Tests: `products::conformance_dynamic` (the dynamic subset) links with Liella's archive and checks each link map
  (`-Map`: Liella's archive is loaded, no host `libc.so.6`/`libc.so`/`libc_nonshared.a`/`libc.a`).
  `s01_startup_symbols::s1_2_c_5_nonshared_archive_two_way` (`products::nonshared_two_way`) links a program and a
  shared object with `-L target/liella -lc` only. It checks the maps and `readelf -V` (only `GLIBC_*` needed), then runs
  both on the reference: the program's atexit and atfork handlers run; the reference's dlclose runs the object's
  atexit handler and drops its quick-exit and atfork handlers.
* Spec: spec/behaviour/dynlink.md, "The link-time archive libc_nonshared.a" (the probe-settled list and the tag).

## Probe-settled (black box, mei; job `dl-libc-nonshared-probe`; probe sources in the story's scratch, not the tree)
Archive members `atexit.oS at_quick_exit.oS pthread_atfork.oS stack_chk_fail_local.oS`; every defined name is hidden,
and `pthread_atfork` is weak. The tag is the VALUE of `__dso_handle`: in `-no-pie` it is 0, so finalize(address) leaves
all three registered. The reference's `__cxa_finalize(dso)` runs the exit handlers and drops the quick-exit and atfork
handlers of `dso`.

## Open questions / next steps
* **Liella gap (INFORMATIONAL):** Liella's `__cxa_finalize(dso)` does not drop atfork handlers
  (`crates/boundary/src/fork.rs`: 4-word nodes with no module slot, `register_dso` ignores `dso`). This is not reachable
  until Liella's loader has `dlclose` (phase 14). It needs a module slot and a removal that is safe against a
  concurrent `fork` walk. Suggested story: DL-ATFORK-FINALIZE.
* The start files (`crt1.o`/`Scrt1.o`, `crti.o`, `crtn.o`) on the dynamic subset's link line are still the host's.
  Liella has `liella-crt1.o` for static links only.
* `libc_nonshared.a` and `libc.so` are not in `remap::PRODUCTS` (the path scan). Adding them changes the `remap`
  layout contract's subjects; the members carry no path by construction (relative compile).
* Deprecation of the archive is the owner's intent, with no action until a replacement exists (DL-NONSHARED-OPTOUT).

## Not to redo
The member list and the tag (value, not address) are settled. Do not disassemble the reference's archive.
