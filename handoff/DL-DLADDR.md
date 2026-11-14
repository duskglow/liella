# DL-DLADDR (DL-18): handoff (run 2, DL-DLADDR-2)

**State.** Branch `task/dl-dladdr-2` (continues `task/dl-dladdr`, cut from origin/next 06d7940b1; no merge). HEAD: see
handoff/reports/DL-DLADDR-2.json. Role kanon:~/w/kn1. dladdr, dladdr1 and dlinfo are exported by Liella's libc
(GLIBC_2.34, compat GLIBC_2.2.5 / GLIBC_2.3.3) and answered in `ld.so` (`crates/boundary/src/ld/dlopen/dladdr.rs`).

**Run 2 commits.** `8d2936a0d` regenerate `csys/src/machine/compat_aliases.S` (the only red of run 1's head: static
compat-gen; clippy was already clean); `ba9af9692` + the next commit: conformance programs `s27_dladdr`,
`s27_dlinfo` and helpers `dla_syms`/`dla_run`/`dla_rp` (clause tests `s27_1_c_6`, `s27_1_c_7`); plants file
`handoff/DL-DLADDR-2-plants.txt`; records and docs commit (pending D-27 decisions `records/pending/DL-DLADDR-2-1..5`,
compat `DL-DLADDR-2-6`, differences row, dynlink.md "dladdr and dlinfo", the probe record in dynlink.glibc.txt).

**Results.** Both conformance programs pass on the reference and on Liella (job dl-dladdr-2-conf2, kanon). Clippy
clean (dl-dladdr-2-clippy0, inherited head; run 2 changed no Rust product code). Plants `dl-dladdr-2-plants` (kanon,
`--dir /mnt/zram/claude/DL-DLADDR-2/plants-1`): at hand-back 5 of 9 CAUGHT with their msg (nearest-off-by-one,
equal-starts-highest-index, linkmap-wrong-object, tls-modid-not-module-table, tls-data-wrong-block), 4 still running
(about 9 min each: every plant rebuilds the products). Gate (`affected --run --base origin/next`) NOT RUN: the role was
held by the plants job (sync rc 4 BUSY), and run 2 stopped at the context checkpoint. dl-dladdr-2-static1 is
SUPERSEDED (it ran on the tree before the records commit because that sync was BUSY).

**Decisions in force.** D-27.1 (readers take no lock), D-27.39 (dlopen'd TLS in the static surplus), the five pending
records above (symbol rule; lock-free readers; dlinfo requests and failures; SERINFO sizing; TLS_DATA per ruling
#5521, with registered compat option `DL-DLADDR-2-6` and a docs/differences-from-glibc.md row).

**Probe-settled.** spec/behaviour/dynlink.md "dladdr and dlinfo" (table) and the `== dladdr-dlinfo` record at the end of
dynlink.glibc.txt. Not recorded on purpose: the probe's forked special-handle and program-ORIGIN cases (see the
hand-back).

**Open / INFORMATIONAL.**
- The vDSO is not in Liella's chain (P-12): `dladdr` of a vDSO address returns 0 (the reference finds `linux-vdso.so.1`).
- Liella's `dl_iterate_phdr` lists only the program and the vDSO (DL-16c walks the chain); the tests take a helper's
  base from its file instead.
- In the conformance build `dlsym(RTLD_DEFAULT, "__tls_get_addr")` is NULL under Liella (ld.so not in the global
  scope); the reference finds it. The test reaches `__tls_get_addr` through the helper.
- `RTLD_DI_SERINFO` on the program: the reference's list leaves out the program's own `DT_RUNPATH` (probe `/prog/rp`);
  Liella's `serinfo` uses the program's scope, so it likely lists it. Not checked (the harness has no program link
  flags directive). Next: probe the program with DT_RPATH and with DT_RUNPATH, then align `serinfo` for `i == 0`.
- `RTLD_DI_ORIGIN` of the program and of ld.so is refused (`dlinfo: the origin of the object is unknown`); giving the
  program's directory (AT_EXECFN) is a possible follow-up; the reference's answer is not recorded.
- When DL-DLOPEN-CONCURRENT (DL-16c) lands: fold its reader into `read_section`/`View` in `dlopen.rs` (or this into it).

**Do not redo.** The probe (tools/probe/dl/dladdr/) and its record; the compat-gen regeneration; the conformance programs.

**Exact next step.** Read the plants job (`~/bin/jobwait dl-dladdr-2-plants` on kanon; all 9 must be CAUGHT), sync, run clippy and `cargo xtask static` on the head, start the gate `cargo xtask affected --run --base origin/next` (escalates to FULL: compat_aliases.S changed). If the train is red on this branch, fix with the direct tests (`cargo xtask affected --only
conformance:s27_dladdr,conformance:s27_dlinfo --with-exports --run`). Then the SERINFO-of-the-program probe above.

**Plan rows.** plan/ph14/design.md §8, §10; plan/ph14/dl-16-dlopen.md §4; D-27.1, D-27.30, D-27.32, D-27.39; dynlink.md P-6, P-12.
