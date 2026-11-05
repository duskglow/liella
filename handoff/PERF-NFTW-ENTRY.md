# PERF-NFTW-ENTRY (+ -2, -3) (branch task/perf-nftw-entry-3, role kn1 on kanon)

State: gain kept, parity NOT reached. nftw per entry (validation/percall/nftw_entry.c, 41 entries/call, instructions:u):
772 -> 694 (one Stat per walk, run 1) -> 573.3 (list() one record pass, reused directory buffer, no name copy/rescan, run 2,
measured in -3: job perf-nftw-entry-3-percall3) -> 532.8 (run 3: per-directory path prefix set once, directory descriptor
kept between entries; job perf-nftw-entry-3-percall4). glibc 228.6, so 2.33x in this shape. The -2 counts jobs never existed
in a jobs.tsv (nothing to collect); -3 re-ran them on kanon (the percall needs `. ~/.cargo/env` and `--base none` there).
Run 3 changes (walk.rs): Frame.prefix + set_name (only the name is replaced after a directory's first entry);
dir_fd cache in walk_in, cleared before levels.push and after levels.pop (eviction under EMFILE can close an ancestor's fd).
Test added: s13_3_i_0 block "two deep chains below one directory" (descriptor limit injected; order-independent).
Plants (nftw3-plants.sh, tests s13_filesystem_clauses + enomem_fs_walk; fstree_products needs built products, left to the gate):
set-name-no-truncate, prefix-off-by-one, dir-fd-level-ignored, no-pop-invalidate; no-push-invalidate is EQUIVALENT by
design (the pop clear covers every level change reachable with the limit injected; the push clear guards EACCES after EMFILE).
Saturation register: docs/saturation/13 (13.13) now has the per-entry breakdown and the open candidates.
Not to redo: Stat reuse, one-pass list(), prefix/fd cache. fts_physical_1000 and the bench rows NOT re-measured.
Next step (each small, ~25-60/entry): keep the listing as raw dirent records (no second copy + Item push); hand the C
callback the walk's Stat without the intermediate copy (fs_tree::run closure 37 + nftw closure 24); reuse one Listing
allocation across directories. Parity at 1.05x (about 240/entry) needs essentially all three plus a leaner examine; decide
whether the story is worth it (the bench rows are 1.10/1.08 and dominated by the status syscall).
Plan rows: none new.

## NOT READY (run -3 end): plants unresolved
Plants job perf-nftw-entry-3-plants3 exits 1: dir-fd-level-ignored and no-invalidate SURVIVED, i.e. the new s13_3_i_0
block does NOT exercise a stale cached descriptor (the injected EMFILE at openat 6 does not leave a closed ancestor fd that
the next entry then uses). Also plants-2 vs plants-3 disagree (dir-fd-level-ignored CAUGHT in -2, SURVIVED in -3;
no-push-invalidate CAUGHT in both although the code says it is redundant with the pop clear): the CAUGHT results came from
the test s13_8_c_0 ("relative to the working directory", a getcwd-style test) failing in plants-2/-3, which looks like an
intermittent failure in a test that depends on the process cwd (FTW_CHDIR tests share the process) - NOT proven, not
investigated for lack of budget. Next step: (1) find why s13_8_c_0 fails intermittently (stress it on its own); (2) either make
the new test hit a stale fd (inject EMFILE on the deepest empty directory's openat) or drop the dir_fd cache (keep Frame.prefix,
worth about 20/entry); (3) rerun plants on a new RAM copy; the gate job is perf-nftw-entry-3-gate2 (see jobs.tsv on kanon).
