# PERF-NFTW-ENTRY-10 (continues -9; fts_physical_1000; branch task/perf-nftw-entry-10, role mei s2)

Verdict: the hot/cold Node split is built and correct, but it moves the count by about 1 instruction per entry: the Node
initialisation was not where the remaining cost is. fts_physical_1000 stays at 1.053 (target 1.05 not reached); nftw_phys_1000 1.02 (kept).

Change (crates/legacy/src/fts.rs, commit "fts: keep the directory-only fields..."): `arg`, `children`, `next_child` moved to
`Cold`, a side vector `Fts::colds` (free list `free_colds`), named by `Node::cold` (0 = none, else index+1). Made on demand
(`cold_mut`): for roots in new_node, for directories when children are built/failed. `root_dev` stayed in Node (a directory
restat'ed from a link becomes a directory later, its root_dev must exist; one store). Node lost 8 words of initialisation.

Counts (mei, Skylake-SP; jobs pne10-percall2, pne10-counts):
- percall fts_tree_1000 (1011 entries) user instr/entry: 1225.8 (-9 head) -> 1224.6 (glibc 750.6). -1.2/entry.
- cbench_fstree instructions: fts_physical_1000 6578662.4 vs glibc 6246998.2 = 1.053 (the -9 CI row was 6692747.9 / 6351574.5 =
  1.054 on another host; the glibc column differs by host, the ratios are the comparable figure); nftw_phys_1000 5708561.2 vs
  5582121.8 = 1.02. Time rows 1.02 / 1.00.
Gate: pne10-gate rc 0 (clippy --all-targets --all-features -D warnings, cargo xtask static, affected --run, peak 2.9G).
Plants (handoff/PERF-NFTW-ENTRY-10-plants.txt, pne10-plants, through cargo xtask plants): 3 CAUGHT (next-child-stuck by the
timeout, arg-empty, slash-root), 1 SURVIVED: cold-children-kept-after-free (not resetting a freed Cold): equivalent in
behaviour: a reused Cold is read only through children/next_child that `build` rewrites before any read, and `arg` only for
roots (set on creation); the reset only releases memory early. I did not re-run to mark it equivalent: the file keeps the
line as run; mark it `equivalent:` if the plants file is run again.

Why no gain, and what is left (per-entry costs in the -9 trace of 10 entries: build closure ~230, new_node ~160, stat_entry ~70,
internal_move ~50, malloc ~44, heap refill, link 27, TryVec push 22): the stores of the dropped fields are cheap
next to the status conversion (stat_bytes: 24 field writes) and the allocation; the remaining 470 instr/entry gap to glibc is
spread. next: (a) write the kernel statx/stat record's bytes straight into the block instead of Stat -> stat_bytes (the kernel
layout and the block's status layout may be near equal; would remove the conversion, ~100); (b) Option<Key> in Node
(32 B) to dev/ino/nlink read from the block. Recommendation on this commit: it is neutral and adds ~100 lines; revert it if
(a) is not going to use the side vector.
