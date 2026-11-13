# DL-DLOPEN-LOAD (DL-16a): handoff (run 4, DL-DLOPEN-LOAD-4)

**State.** Branch `task/dl-dlopen-load-4` (runs 1-3 included, origin/next merged in). Role shiki:~/w/sh2. dlopen and
dlerror work end to end (kernel-started and `ld.so PROGRAM`), now beside DL-15's per-thread TLS. The records are
assigned (COMPAT-23..26, D-27.37; the coordinator, ruling #5320). The branch can land once the run-4 jobs are green.

**Run 4 (merge only, as the brief asked).** `ff86663bc` merges origin/next (2f8ae2e64). The conflicts were in
`crates/rt/src/start.rs` (StartInfo), `crates/boundary/src/ld.rs` (run_init builds it) and
`crates/boundary/src/runtime.rs` (early_init reads it). Both changes are kept: next's
`tls_size/tls_align/tls_init/tls_release` stay where DL-15 put them, `dlopen` comes after them (the order is free:
only Rust reads the struct, and the build tag ties ld.so and libc to one build), and early_init calls
`set_loader_tls` and then stores `LOADER_DLOPEN`. Checked beside the conflicts: `crates/ld/src/reloc.rs` merged
cleanly (DL-15's TLSDESC arm and DL-16a's two-pass split touch different lines); the thread block's `dlerror` word
(0x280) is not touched by next; a reused thread mapping is zeroed from the block start (thread.rs), so a new thread's
`dlerror` word starts at 0. `221137a27` adds the plants file `handoff/DL-DLOPEN-LOAD-4-plants.txt`.
`61755515a` makes the CONF-DYN-HELPERS conformance flip, which the merge brought in and the gate asked for: the
merged head passed `s27_dlopen_failures` on Liella, so run.sh failed it as a PENDING-LIELLA program that now passes
(the designed signal). The directive is gone, and the clause test `s27_1_f_0` (crates/core/tests/s27_dynlink_clauses.rs)
asserts the "1 passed, 0 failed" summary. `s27_dlsym_handles` and `s27_dynlink_helper_dlsym` stay pending on DL-16b.
Plants (10): two break one half each of the merged handover, one cuts a dlerror text the flipped corpus checks, and
the seven run-3 plants re-run.

**Jobs (shiki).** `dl-dlopen-load-4-lint0`: clippy and static on the inherited head f2f4d0e81, rc 0.
`dl-dlopen-load-4-t1` on ff86663bc: workspace clippy (all targets and features, -D warnings), `cargo xtask static`,
`cargo xtask so`, dl_tls_threads 2/2, dlopen 2/2, dlopen_rules 3/3, no SKIP, rc 0. Superseded, on 221137a27:
`-gate` (escalated to FULL by the branch's diff: dl.S, compat_aliases.S, xtask/src/compat.rs) rc 1, 6509 passed / 1
failed, and that failure was the flip above; `-plants` 9/9 CAUGHT, rc 0. `-t2` on 61755515a: clippy, static, so,
s27_dynlink_clauses 2 passed, rc 0. Final jobs on 61755515a, `dl-dlopen-load-4-gate2` and `dl-dlopen-load-4-plants2`:
results in `handoff/DL-DLOPEN-LOAD-4-report.md`.

**Open / for later stories.**
- A dlopen'd object that references a start-time object's TLS (for example initial-exec TPOFF against the C
  library's TLS) is refused with "a thread-local storage relocation refers to an object without a TLS segment":
  `adopt_start` (crates/boundary/src/ld/dlopen.rs) rebuilds the start-time objects with `tls: None`. This gap predates
  the merge. DL-15 now records each placed module's ID and TP offset (`THREAD_MODS` in ld.rs), which is what DL-16d
  needs to fill `tls` there.
- The dlerror record is not freed at thread exit: DL-15's `tls_release` call in thread.rs is now the hook (DL-15
  follow-up or DL-16c).
- Capacity of 512 objects per process: the reference was not probed for it (DL-16b or later: probe it and decide).
- Rows 318-321 of `docs/differences-from-glibc.md` still read "pending record `records/compat/COMPAT-2x.toml` (id
  assigned by the coordinator)": the paths are right, the word "pending" is stale (a one-word cleanup).

**Known gaps (later stories, unchanged):** start-time `l_name` = DT_SONAME; TLS of dlopen'd objects (DL-16d); one
`UniqueTable` per call (DL-16b); start-time `$ORIGIN`; requester chain; fork/lock (DL-16c); static `dlopen(NULL)`
gives the static text with an empty name (DL-16S).

**Do not redo:** the probes, the driver, START_ARGS, the selector analysis (an ungated boundary module must not name
a liella-ld item), the records, this merge.

**Next step:** land; then DL-16b (dlsym) stacks on this branch.

**Plan rows to read:** plan/ph14/dl-16-dlopen.md §1-§4, §8; D-27.7, D-27.30, D-27.31, D-27.34, D-27.37; design §7
(DL-15's thread TLS); spec/behaviour/dynlink.md "dlerror texts" and P-10, P-13.
