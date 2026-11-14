# DL-DLADDR (DL-18): handoff (run 3, DL-DLADDR-3)

**State.** Branch `task/dl-dladdr-3` (contains all of `task/dl-dladdr-2`, which continues `task/dl-dladdr`, cut from
origin/next 06d7940b1; no merge). HEAD: see handoff/reports/DL-DLADDR-3.json. Role kanon:~/w/kn1. dladdr, dladdr1 and
dlinfo are exported by Liella's libc (GLIBC_2.34, compat GLIBC_2.2.5 / GLIBC_2.3.3) and answered in `ld.so`
(`crates/boundary/src/ld/dlopen/dladdr.rs`). The work is done and verified; the branch is ready to land once the
coordinator assigns ids to the pending records below.

**Earlier commits (runs 1-2).** The code; `8d2936a0d` regenerated `csys/src/machine/compat_aliases.S`; conformance
programs `s27_dladdr`, `s27_dlinfo` with helpers `dla_syms`/`dla_run`/`dla_rp` (clause tests `s27_1_c_6`, `s27_1_c_7`);
records and docs (pending D-27 decisions `records/pending/DL-DLADDR-2-1..5`, compat `DL-DLADDR-2-6`, a
docs/differences-from-glibc.md row, dynlink.md "dladdr and dlinfo", the probe record in dynlink.glibc.txt).

**Run 3.** Run 2's plants job `dl-dladdr-2-plants` ended rc 124 with 8 of 9 CAUGHT: the cause is the 150-min cap
on shared jobs (`~/bin/slots`, SLOT_MAX_MIN), not a test. Each plant costs about 16.5 min: the plants tool rebuilds
the products with the mutant (~8.7 min), then the plant's command (`cargo xtask affected --only conformance:...
--with-exports --run`) builds them again (~7.2 min, "433.5s ok products" in each plant log). Nine plants plus the
baseline need about 167 min. Run 3 split the same nine plants, unchanged, into `handoff/DL-DLADDR-3-plants-a.txt`
(5) and `-b.txt` (4, the unjudged phdr-count-short first) and ran them as two jobs. No product code changed.

**Results (head 5d7461be1, kanon).**
- `dl-dladdr-3-clippy0`: `cargo xtask clippy` + `cargo xtask static` on the inherited head ef6eacb2d: rc 0.
- `dl-dladdr-3-gate`: `cargo xtask affected --run --base origin/next`, escalated to FULL (compat_aliases.S is
  generated): rc 0; suites 32 ok, tests 11767 passed / 0 failed / 1269 ignored; conformance 131 passed / 0 failed;
  peak 16G (page cache included), oom_kill 0.
- `dl-dladdr-3-plants-a` (`--dir /mnt/zram/claude/DL-DLADDR-3/plants-a1`): rc 0, 5 CAUGHT, 0 SURVIVED, 0 WARN, 103 min.
- `dl-dladdr-3-plants-b` (`--dir /mnt/zram/claude/DL-DLADDR-3/plants-b1`): rc 0, 4 CAUGHT (phdr-count-short included),
  0 SURVIVED, 0 WARN, 87 min. All nine plants are caught by the msg each names.
- SUPERSEDED: `dl-dladdr-2-plants` (rc 124 at the cap, 8 of 9 judged), `dl-dladdr-2-static1` (older tree).

**Decisions in force.** D-27.1 (readers take no lock), D-27.39 (dlopen'd TLS in the static surplus), the five pending
records (symbol rule; lock-free readers; dlinfo requests and failures; SERINFO sizing; TLS_DATA per ruling #5521, with
the registered compat option `DL-DLADDR-2-6` and the differences row).

**Probe-settled.** spec/behaviour/dynlink.md "dladdr and dlinfo" (table) and the `== dladdr-dlinfo` record at the end
of dynlink.glibc.txt.

**Open / INFORMATIONAL (carried from run 2, not in this story's scope).**
- The vDSO is not in Liella's chain (P-12): `dladdr` of a vDSO address returns 0 (the reference finds `linux-vdso.so.1`).
- Liella's `dl_iterate_phdr` lists only the program and the vDSO (DL-16c walks the chain); the tests take a helper's
  base from its file instead.
- In the conformance build `dlsym(RTLD_DEFAULT, "__tls_get_addr")` is NULL under Liella (ld.so not in the global
  scope); the reference finds it. The test reaches `__tls_get_addr` through the helper.
- `RTLD_DI_SERINFO` on the program: the reference's list leaves out the program's own `DT_RUNPATH` (probe `/prog/rp`);
  Liella's `serinfo` uses the program's scope, so it likely lists it. Not checked. Follow-up: probe the program with
  DT_RPATH and with DT_RUNPATH, then align `serinfo` for `i == 0`.
- `RTLD_DI_ORIGIN` of the program and of ld.so is refused; giving the program's directory (AT_EXECFN) is a possible
  follow-up; the reference's answer is not recorded.
- When DL-DLOPEN-CONCURRENT (DL-16c) lands: fold its reader into `read_section`/`View` in `dlopen.rs` (or this into it).

**Do not redo.** The probe (tools/probe/dl/dladdr/) and its record; the compat-gen regeneration; the conformance
programs; the plants (all nine judged in run 3).

**Exact next step.** Coordinator: assign ids to `records/pending/DL-DLADDR-2-1..6`, then land. A follow-up story may
take the SERINFO-of-the-program probe above.

**Plan rows.** plan/ph14/design.md §8, §10; plan/ph14/dl-16-dlopen.md §4; D-27.1, D-27.30, D-27.32, D-27.39; dynlink.md P-6, P-12.
