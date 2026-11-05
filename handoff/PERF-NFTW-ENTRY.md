# PERF-NFTW-ENTRY (+ -2, -3, -4) (branch task/perf-nftw-entry-4, role n1 on keke)

State: gain kept, parity NOT reached, plants resolved. nftw per entry (validation/percall/nftw_entry.c, 41 entries/call,
instructions:u): 772 -> 694 (one Stat per walk) -> 573.3 (one-pass list(), reused buffer) -> 532.8 (per-directory path prefix set
once, directory descriptor kept between entries). glibc 228.6, so 2.33x in this shape. The bench rows are 1.10/1.08 and are
dominated by the status syscall. Run -4 did not re-measure (no product code changed in -4).

Run -4 (cause of the red -3 plants job found and fixed):
* The CAUGHT/SURVIVED results of the hand-written plants script came from a flake, not from the plants: s13_8_c_0 compares
  realpath(".") with the kernel's working directory, while s13_3_c_4 and s13_3_i_0 and fts_physical-style tests open fts in
  chdir mode IN the test process (no FTS_NOCHDIR), which changes the shared working directory until fts_close (and Scratch
  removal can delete it). It failed in 3 of 10 runs on UNMUTATED code; 0 of 30 after the fix (one `cwd_lock()` in
  s13_filesystem_clauses.rs taken by the four sites).
* Plants moved to the shared runner: handoff/PERF-NFTW-ENTRY-4-plants.txt (job perf-nftw-entry-4-plants2: 2 CAUGHT,
  3 EQUIVALENT, 0 SURVIVED). set-name-no-truncate and prefix-off-by-one are caught by the s13 tests.
  dir-fd-level-ignored, no-push-invalidate and no-invalidate (both clears removed) are EQUIVALENT: the level compare alone
  guards the cache (a directory is always examined at its parent's level between two stretches at one level, and push
  never evicts the top level's descriptor), so the clears are defensive. A sibling-eviction test written to catch them could
  not be shown to catch anything and was dropped.
* Nothing else changed in product code. Saturation register 13.13 has the remaining cost and candidates.

Not to redo: Stat reuse, one-pass list(), prefix/fd cache, the cwd lock.
Next step (only if the owner wants parity; each ~25-60/entry): keep the listing as raw dirent records (no second copy + Item
push); hand the C callback the walk's Stat without the intermediate copy (fs_tree::run closure 37 + nftw closure 24); reuse one
Listing allocation across directories; then a leaner examine. Parity at 1.05x (about 240/entry) needs all of them.
Plan rows: none new.
