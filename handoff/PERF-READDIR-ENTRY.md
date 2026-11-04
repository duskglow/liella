# PERF-READDIR-ENTRY (branch task/perf-readdir-entry-2, continued from task/perf-readdir-entry)

State: code commit 8dee0d52 on top of cdf9880a (lock_alone swap, record-walk tests) and 232388b7. Stacked on
task/perf-fswalk-phys, which is not in next yet. Role keke ~/w/n4. Run 2 (PERF-READDIR-ENTRY-2).

Run 1 jobs (keke, all rc 0): perf-readdir-entry-percall, PERF-READDIR-ENTRY-static, perf-readdir-entry-gate,
perf-readdir-entry-plants (3 record-walk plants CAUGHT). Run 1's total instructions per entry: readdir_entry_flat
Liella 1630.7 -> 1640.8, glibc 1636.6 -> 1640.3 (kernel getdents64 dominates; both sides moved by the same amount,
so treat it as noise); fts_entry_flat 6930 -> 6945 vs glibc 8044/8049; nftw_phys_1000 6271807 -> 6270280;
fts_physical_1000 and the walk rows unchanged within 0.1%. Run 1 counted totals only, not user-mode instructions.
Its `readdir64.dis` came out empty: readdir and readdir64 share one body, the symbol to read is `<readdir>`.

Run 2 change (8dee0d52): `<readdir>` was about 45 instructions on a hit, with `sub/add $0x18,%rsp` (the slow
path's KResult went through the stack), `mov $0x16,%eax` (EINVAL hoisted onto the hit path) and a register copy.
The exports `readdir`/`readdir64` now call `dirtree::readdir_addr(d) -> usize` (the address, 0 at the end or on
an error). Every miss (bad handle, lock_alone false, empty buffer, bad record) is one tail call to
`readdir_cold`, which checks the handle again, runs `readdir_slow` and sets errno only on an error. The record
walk (`DirStream::buffered_addr`) and its bounds checks are unchanged. Measured (perf-readdir-entry-2-percall, user-mode
instructions per entry, 3 runs per side, spread 0): readdir_entry_flat 50.293 -> 46.324, glibc 46.122 (ratio
1.09 -> 1.004; the 54.3 recorded in the register predates PERF-FSWALK-PHYS and the lock_alone swap). fts_entry_flat
1493.19 -> 1493.19 (glibc 793.6, unchanged). Totals unchanged within noise: opendir_readdir_closedir_1000,
fdopendir_99, scandir_alphasort_1000, nftw_phys_1000 6271364 -> 6269368, fts_physical_1000 6877174 -> 6876117,
fts_nochdir_flat_1000 6905129 -> 6917627 (+0.2%, kernel-dominated total; glibc moved by the same amount: 7960554 ->
7969779). The hit path is 41 instructions in after/readdir.dis. Plants: 3/3 CAUGHT (sym_readdir lines 1801 and
1804; fast-skips-record by s13_filesystem_clauses). Static (clippy -D warnings + xtask static): rc 0. Gate:
perf-readdir-entry-2-gate rc 0 (5 suites ok, 2192 passed, 0 failed; conformance 2/2).

Run 2 jobs (keke ~/logs/jobs.tsv; driver ~/work/perf-readdir-entry-2/driver.sh, log driver.log):
- perf-readdir-entry-2-percall: in n4, `patch -R` change.patch -> count.sh before -> `patch` -> count.sh after.
  Output in ~/work/perf-readdir-entry-2/{before,after}/: cbench.log, cbench_fstree.log (totals table),
  ctr.tsv (the binaries run directly with CBENCH_COUNTERS=1, 3 runs per side; the 7th field is user-mode
  instructions), readdir.dis.
- perf-readdir-entry-2-static: workspace clippy -D warnings, then `cargo xtask static`.
- perf-readdir-entry-2-gate: direct tier, --base origin/next.
- perf-readdir-entry-2-plants: RAM copy /dev/shm/claude/PERF-READDIR-ENTRY-2/plants-1. Plants: no-errno (cold
  path drops errno), end-sets-errno (the end sets errno), fast-skips-record (the hit path throws its record
  away). Tests: liella-boundary s13_filesystem_symbols (readdir filter) and liella s13_filesystem_clauses, each
  under timeout 600. Exits 1 on a survivor or a plant that did not apply.

Open / next step:
1. The readdir user-instruction gap is closed (1.004). The remaining fts_entry_flat gap (1.88x user) is not a
   readdir question: a separate story. The register paragraph in docs/saturation/17 records the counts.
2. What is left on the hit path (after/readdir.dis): null and alignment test of the handle (setcc/or form), the
   magic compare (movabs), the `Option<DirStream>` niche test, and in buffered_addr the `start <= end` compare
   of `get(base+pos..base+len)` (it cannot fail once pos < len, but LLVM cannot see that through wrapping_add)
   plus the `rec.len() >= 18` check. Slicing `buf.get(base..)` once and comparing the record length against
   `len - pos` could save 2-3 more, but that is a change to the record walk: rerun run 1's three plants on it.
Not to redo: the lock_alone swap (done, in force), the record-walk tests (done).
Plan rows to read: DU-3, MUTEX-2.
