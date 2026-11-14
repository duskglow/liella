# SMOKE-ERRNO-FIXES: readdir on a removed directory, setlocale's errno on failure (AQOURS-SMOKE F1, F3)

State: branch task/smoke-errno-fixes-2 (continues task/smoke-errno-fixes); code head 39c95fe86 + a5e3b8f57
(fixes), b811e98b8 (rulings applied to spec text and differences row); this note and the report json follow.
Role keke:~/w/n3 (SMOKE-ERRNO-FIXES-2); the first run used kanon:~/w/kn1.

## What changed
* F1 (ruling #5467): `DirStream::refill` (crates/core/src/fs/dir.rs) takes `getdents64`'s `ENOENT` as the end
  of the directory. `readdir`/`readdir64` return NULL with errno unchanged, `readdir_r`/`readdir64_r` return 0,
  `scandir` returns what it read. Every reader goes through `refill` (`next`, `next_addr`, `next_named`,
  `next_chunk`): readdir forms and scandir (crates/boundary/src/dirtree.rs), nftw/ftw (crates/core/src/fs/walk.rs),
  fts (crates/legacy/src/fts.rs), glob (crates/core/src/pattern/glob.rs), getcwd's parent scan
  (crates/core/src/fs/cwd.rs; still ENOENT, from its own not-found result). Direct `getdents64` callers stay as
  they were (the `getdents64`/`getdirentries` exports still return ENOENT, as the reference; internal scans).
  fstree_products: the 19 D-13.2 known-difference rows are removed. The differences row is removed.
* F3 (ruling #5469): `setlocale` returning NULL sets ENOENT (a name; as newlocale, D-7.18) or EINVAL (unknown
  category), in a cold out-of-line helper; success leaves errno alone. Probe rows: spec/behaviour/setlocale-errno.md.
* Rulings applied (SMOKE-ERRNO-FIXES-2): #5479, clause s13_2_f_0's text (glibc-functional-spec.md §13.2 Failure,
  spec/clauses.tsv regenerated with `cargo xtask stubs`) now says removal ends the stream with errno unchanged;
  #5480, the setlocale differences row has no compat option and says why (the reference's errno on success depends
  on process history, so no deterministic behaviour for an option to select).
* Decisions: records/pending/SMOKE-ERRNO-FIXES-1.toml (D-13 series, amends D-13.2), -2.toml (D-7 series):
  the coordinator assigns their ids.

## Jobs (keke, head b811e98b8): LAYOUT-JITTER, story held
smoke-errno-fixes-2-gate (clippy + static + affected --run --base origin/next): clippy and static green; rc 1 at
`cargo xtask test`'s layout step: STARTUP-DATA-WINDOWS 13 written pages > rw-budget-fast 12 (pages alone:
vdso_fn's .data.rel.ro, printf `text_only::C`, heap RESERVE_MEM x2). smoke-errno-fixes-2-plants (3 plants,
`--timeout 2400`): the build finished in time (the 600 s timeout cause is gone), but the baseline `so` failed on the
same contract (rc 2), so no plant was applied.
Measured in the same role (keke ~/w/n3, fast build): origin/next 3e7723955 alone, job smoke-errno-fixes-2-next-so
rc 0: 11 written pages (budget 12), 1 read-only window. This branch's diff applied on 3e7723955 (scratch tree, not a
merge on the branch), job smoke-errno-fixes-2-onnext-so rc 1: 13 pages, same four objects. The code diff touches no
start-up code (a `#[cold]` `setlocale_failed` helper and one match arm in `DirStream::refill`); new cold code moving
the start-up data through the page grid is the mechanism in handoff/layout-stability-rethink.md. Not chased (rule
#4347: no regen loops, no section moves, no bound change).
Next step: once the start-up layout fix lands (trace-at-build grouping; LAYOUT-RETHINK), re-sync, run the gate
and `cargo xtask plants handoff/SMOKE-ERRNO-FIXES-plants.txt --dir <new RAM dir> --timeout 2400`; nothing else to change.
Diagnostic: smoke-errno-fixes-2-stubs (keke; regenerated clauses.tsv; rc 128 from a trailing `git status` in a
role dir without .git, the regeneration itself completed). First run: smoke-errno-fixes-plants3 (kanon) rc 2:
its baseline `so` hit the tool's 600 s default (cold traced build), not the code (PLANTS-PREPARE-TIMEOUT).

## Open
* NOT VERIFIED: the C reproducers (~/work/SMOKE-ERRNO-FIXES/run_probes.sh on kanon) were not re-run after the
  fix; the clause tests carry the same cases. The coreutils `removed-directory` case as a conformance program is
  not written. Both are optional follow-ups, not blockers.

## Do not redo
Probes of the reference (first run, job smoke-errno-fixes-probe0 on kanon): readdir_removed NULL errno 0;
setlocale unknown name NULL errno 2; scandir with a removing filter n=5 errno 0; readdir_r returns 0.
