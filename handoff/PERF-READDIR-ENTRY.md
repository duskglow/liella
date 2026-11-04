# PERF-READDIR-ENTRY (branch task/perf-readdir-entry)

State: code commit cdf9880a (stacked on task/perf-fswalk-phys, which has not landed in next yet; the branch was
cut from it by the dispatcher). Jobs started detached on keke role r5, NOT VERIFIED at hand-back (budget: the
host's job slots were full; my first baseline job waited 11 min without starting).

Change:
- `crates/boundary/src/dirtree.rs` `readdir` fast path asks `thread::lock_alone()` (one relaxed load, the MUTEX-2
  form) instead of `thread::alone()` (two acquire loads and a branch). lock_alone implies alone (set from
  `__libc_single_threaded` only once Liella owns the process, cleared for good by the first thread or shared
  task); when it is false the slow path asks `alone()` itself, so locking is unchanged in every case.
- `crates/core/src/fs/dir.rs`: new unit tests `buffered_addr_walks_each_record_once` and
  `buffered_addr_rejects_bad_record_lengths` (d_reclen 0/8/19 and a record past the returned bytes -> EIO).
  Before this there was no test that a short or overlong `d_reclen` is rejected.
- The record walk's bounds checks are unchanged.

Jobs (keke ~/logs/jobs.tsv), run in sequence by `~/work/perf-readdir-entry/driver.sh` (pid in driver.pid):
1. `perf-readdir-entry-percall`: `count.sh before` in r5 (old tree), `git apply change.patch` (= this commit's
   code), `count.sh after`. Output in `~/work/perf-readdir-entry/{before,after}/` (cbench.log =
   cbench_unmeasured incl. readdir_entry_flat and fts_entry_flat; cbench_fstree.log = nftw/fts walk rows;
   readdir64.dis = Liella's readdir64 disassembly for the next step).
2. `PERF-READDIR-ENTRY-static`: workspace clippy -D warnings + `cargo xtask static`.
3. `perf-readdir-entry-gate`: direct tier, `--base origin/next`.
4. `perf-readdir-entry-plants`: plants.sh, 3 plants on `buffered_addr` (skip-record, short-reclen, past-buffer),
   exits 1 on a survivor. The lock_alone swap has no functional plant: it is equivalent by design for every
   single-threaded test (it only changes which of two lock-free paths is taken).
CAVEAT: count.sh uses `cargo xtask bench --compare --counters <bench>` from CLAUDE.md, flag not checked on the
host. If `percall` is red, the patch was NOT applied and jobs 2-4 tested the old tree: treat them as superseded.

Open: whether the lock_alone swap closes enough of the 54.3 vs 46.1 user-instruction gap (estimated 2-3 per
entry, not measured). Not to redo: nothing measured yet.
Next step: read `after/cbench.log` vs `before/cbench.log` (keep only if no row slower), then use
`before/readdir64.dis` to find the rest (candidates: slot magic/alignment check, `Option<DirStream>` test,
`ret::entry` match, the two slice bound checks in `buffered_addr`). Update docs/saturation section 17 with the rows.
Plan rows to read: DU-3, MUTEX-2 (grep plan/07-decisions).
