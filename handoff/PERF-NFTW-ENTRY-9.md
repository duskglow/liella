# PERF-NFTW-ENTRY-9 (continues -8; fts_physical_1000; branch task/perf-nftw-entry-9, role keke n1)

State: percall fts_tree_1000 (1011 entries, user instr/entry): 1387.2 (the -8 head, recounted) -> 1225.8 (glibc 753.6),
-161/entry (-11.6%); ratio 1.84 -> 1.63. The percall ratio is not the bench row's ratio (the row also holds the fixed per-directory
and open/close costs): the bench row `cbench_fstree` / fts_physical_1000 was recounted AFTER the changes only (job
perf-nftw-entry-9-bench2; the -8 head's row was not recounted: the first attempt used the wrong name and a queue wait ate the
budget). Reference rows: V-4 1.09, -7 1.078. RESULT (bench2, final head): fts_physical_1000 instructions 6692747.9 vs 6351574.5 = 1.05
(target 1.05 reached by the printed ratio; 1.0537 unrounded); nftw_phys_1000 1.02; time rows 1.02 / 1.00.

Steps, each counted (percall, same case):
1. stat_entry writes into one reused probe (kernel writes straight into it; no 144 B status inside a tuple moved twice) and
   new_node assigns the Node in its slot: 1387.2 -> 1272.8 (-114).
2. The FTSENT header and stat_bytes write through a `&mut [u8; N]` (`putf`): each field write loses its bounds check:
   1272.8 -> 1241.7 (-31; put_header was 97/entry).
3. The name's padding and the status are one part of the block fill (LEAD zeros ahead of the status array): 3 variable
   copies (internal_move, ~25 each) -> 2: 1241.7 -> 1225.8 (-16).
Not done (the design the ruling proposed, still open): a small hot Node with the cold fields (arg, children, next_child,
root_dev) in a side vector for directories only (~45 sites in fts.rs); the Node is still ~200 B. Remaining per-entry
costs in the trace of one directory build (10 entries): build closure ~230, new_node ~160, stat_entry ~70, internal_move
~50 now, malloc ~44, heap refill, link 27, TryVec push 22.

Profile that guided step 2/3 (wl_trace, fts_read call 2, 11775 instr/10 entries): closure 2316, new_node 1600, put_header 970,
refill 808, internal_move 738, stat_entry 710; the internal_move callers were three calls in new_node (name, pad, status).

Plants: handoff/PERF-NFTW-ENTRY-9-plants.txt: 6 of 7 CAUGHT; status-size-wrong SURVIVED (no test read fts_statp's size): fixed
with a new test (s13_filesystem_clauses: names of four padding lengths, size/blocks/ino/nlink against stat), re-run on its own:
plants2. Gate (affected): perf-nftw-entry-9-gate green on the code before the test; gate2 on the final head.

Next step: build the hot Node / cold side data (see above); recount the bench row after.
Process notes: edits were applied with python scripts (not the Edit tool) for 3 multi-site changes; jobrun in a chained command
blocks the shell and queue waits were 5-10 min each; `cargo fmt` on the role reformatted the whole file (the repo does not keep
fts.rs rustfmt-clean): never run it on fts.rs.
