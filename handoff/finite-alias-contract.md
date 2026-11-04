# FINITE-ALIAS-CONTRACT (handoff)

State: branch task/finite-alias-contract, code head 4f0778ee (handoff commit on top). Role shiki:~/w/sh1.
Done: s10_0_c2_0 (crates/math/tests/s10_math_clauses.rs) is now a real test. fam_finite covers finite/finitef/finitel
only, so it was not a tag fix. The test parses Liella's built target/liella/libm.so.6 (small ELF64 reader, no tools) and checks
for every __<name>_finite: (1) exported only at its Appendix B version, non-default; (2) its compat-plan.tsv alias row targets
<name>; (3) the stub is one jmp rel32 that reaches <name>@@V. The stubs jump through the PLT (jmp exp@PLT, libm.so.6 is not
-Bsymbolic), so the test follows PLT entry -> GOT slot -> R_X86_64_JUMP_SLOT relocation -> symbol and requires name, default
version and address to equal <name>'s default symbol; a direct jump to <name>'s address is accepted too. (4) the set of _finite
exports equals spec/symbols.tsv section-10 _finite rows (107). Skips via skip_note when libm.so.6 is not built.
Probe-free: reads Liella's own object only.
Decision in force: D-10.2 (finite aliases carry the plain function's contract). __gamma*_r_finite target lgamma*_r (gamma is the
old name of lgamma, gamma(3); no gamma*_r export): the test maps gamma* -> lgamma* for the target name.
Verified in this run: s10_0_c2_0 passes on the role build (1 passed); workspace clippy -D warnings rc 0 on the previous commit
(only comment tags added since). First static run red on arch-audit (2 untagged R_X86_64_JUMP_SLOT lines), fixed by ARCH(x86_64) tags.
NOT VERIFIED at hand-back (budget): static rerun, gate, plants were started detached on shiki (jobs below).
Plants (~/work/finite-alias-contract/plants.sh 1, RAM copy /dev/shm/claude/FINITE-ALIAS-CONTRACT/plants-1): (a) __exp_finite
target=exp2 + compat-gen; (b) drop the __logf_finite row + compat-gen; (c) __sqrt_finite@@GLIBC_2.15 in compat_aliases.S. Plant (c)
may be caught by cargo xtask so's own export check (export_problems ignores @@ names) before the test runs; the script reports
which layer caught it. Exits non-zero on a survivor or an unapplied pattern.
Jobs (shiki): finite-alias-contract-static, finite-alias-contract-gate, finite-alias-contract-plants.
Next step: read the three jobs' rows in shiki ~/logs/jobs.tsv; on a red, fix and rerun only that job.
