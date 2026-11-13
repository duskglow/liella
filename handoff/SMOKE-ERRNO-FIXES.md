# SMOKE-ERRNO-FIXES: readdir on a removed directory, setlocale's errno on failure (AQOURS-SMOKE F1, F3)

State: branch task/smoke-errno-fixes; code head a5e3b8f57 (this note and the report json follow it). Role kanon:~/w/kn1.

## What changed
* F1 (ruling #5467): `DirStream::refill` (crates/core/src/fs/dir.rs) takes `getdents64`'s `ENOENT` as the end
  of the directory. `readdir`/`readdir64` return NULL with errno unchanged, `readdir_r`/`readdir64_r` return 0,
  `scandir` returns what it read. Checked by code: every reader goes through `refill` (`next`, `next_addr`,
  `next_named`, `next_chunk`): readdir forms and scandir (crates/boundary/src/dirtree.rs), nftw/ftw
  (crates/core/src/fs/walk.rs), fts (crates/legacy/src/fts.rs), glob (crates/core/src/pattern/glob.rs),
  getcwd's parent scan (crates/core/src/fs/cwd.rs; still ENOENT, from its own not-found result). Direct
  `getdents64` callers stay as they were: the `getdents64`/`getdirentries` exports (still ENOENT, as the
  reference) and internal scans (process.rs, system.rs, legacy io_compat.rs, boundary locale.rs).
  fstree_products: the 19 D-13.2 known-difference rows are removed (they now match; the test fails on a stale row).
  The row in docs/differences-from-glibc.md is removed.
* F3 (ruling #5469): `setlocale` returning NULL sets ENOENT (a name; as newlocale, D-7.18) or EINVAL (unknown
  category), in a cold out-of-line helper; success leaves errno alone. Probe rows: spec/behaviour/setlocale-errno.md
  (the reference: unchanged errno when the same unknown name fails again, EINVAL for some refused names, ENOENT
  after one successful `setlocale(LC_ALL, "C.UTF-8")`; none copied). New differences row (no compat option, pending ruling).
* Decisions: records/pending/SMOKE-ERRNO-FIXES-1.toml (D-13 series, amends D-13.2), -2.toml (D-7 series).

## Probes (kanon, job smoke-errno-fixes-probe0, before the fix)
readdir_removed: reference `NULL errno 0`, Liella `NULL errno 2`. setlocale_errno: reference `NULL errno 2`,
Liella `errno 0`. scandir with a filter that removes the directory: reference n=5 errno 0, Liella -1 ENOENT;
readdir with errno preset 1234: reference leaves 1234; readdir_r returns 0.

## Jobs (code head 0e5e0872c)
smoke-errno-fixes-gate2 (clippy + static + affected --run --base origin/next), smoke-errno-fixes-plants3
(3 plants, handoff/SMOKE-ERRNO-FIXES-plants.txt). Superseded: smoke-errno-fixes-check (static red: fs-order
on read_dir in the new test filter, records `amends` type), smoke-errno-fixes-gate (fs-order: scandir call
without an FS-ORDER note, fixed in 0e5e0872c), smoke-errno-fixes-plants and -plants2 (killed, older heads).

## Open
* RULING NEEDED: clause s13_2_f_0's text still lists "ENOENT (the directory was removed)" as a readdir error;
  recommend amending the spec text to say the removal ends the stream (spec/behaviour/fstree.md).
* RULING NEEDED: setlocale differences row without a compat option (recommend none: POSIX defines no setlocale
  errors; the reference's value depends on process history).
* NOT VERIFIED: the C reproducers were not re-run after the fix (the check job stopped at static); the clause
  tests carry the same cases. Next step: re-run ~/work/SMOKE-ERRNO-FIXES/run_probes.sh on kanon; the coreutils
  `removed-directory` case as a conformance program is not written.
