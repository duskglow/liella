# PERF-NFTW-ENTRY-8 (continues PERF-NFTW-ENTRY-7; fts_physical_1000 only; branch task/perf-nftw-entry-8, role mei s1)

State: fts parity NOT reached. Bench row fts_physical_1000 was 1.078 (instr, from -7); this run cut ~22 user instr/entry
(percall 1407.5 -> 1385.2 vs glibc 750.6), about 3% of the ~170/entry the row needs for 1.05. The bench row was not recounted
(no budget; the percall case is the measure). Remaining: ~150/entry.

Added: validation/percall/fts_tree1000.c (case fts_tree_1000: the bench shape, a root with 10 dirs x 99 files, 1011 entries;
tree built in setup, cancels in n=0). Note the bench row is 10 directories of 99, not one 1000-entry directory.

Profile (wl_trace, one fts_read building a 99-entry directory, 103527 instr = 1046/entry; steady entry ~976 in the window):
build closure (dirent loop, cycle check, kids push) 247, new_node 198, stat_entry 145 (incl. kernel-stat -> Stat), header 77,
internal_move 76 + memmove 61 (the ~300 B Node moved into Some(node) and into the TryVec, the 144 B status array copy),
TryVec push 24, malloc 44, link 31, stat_bytes 24. Fixed per directory: ~16.9k (open, fchdir, DirStream buffer fill, lseek).
Whole pass per entry (percall): 1385 = build ~1000 + steady fts_read ~335 + post-order frees.

Done (kept, both measured): (1) FTSENT fields written straight into the block (no temporary [u8;112] and 3 copies): -1
instr/entry (model was wrong: copies were inlined); (2) Node slimmed (size/name_cap derived, no empty TryVec built per entry):
-21/entry. Neither makes any measured row slower.

Next step (needs ~150/entry): the cost is spread, no single phase holds it. The biggest single items: (a) build the Node in place
(no 300 B move to Some(node) then push; ~130 of moves/memmove), e.g. store entries in a struct-of-arrays or box-less slab
with a small hot Node (block, parent, level, info, stat key) and the cold fields (arg, children, next_child, follow, root_dev)
in a side vector for directories only; (b) write Stat fields directly to the block instead of stat_bytes + 144 B copy (~60);
(c) merge stat_entry's Stat conversion with it. Read crates/legacy/src/fts.rs new_node/build.

Process notes: two empty heredocs slipped in (no product impact). Percall needs `--base none` on a role without primed base.
