# PERF-NFTW-ENTRY (+ -2) (branch task/perf-nftw-entry-2, role r1 on keke)

State: partial. Stat reuse (run 1, 772 -> 694 instr/entry) kept. Run 2 (commit "Walk listings by one record pass..."):
(1) list(): DirStream::next_named = one pass per dirent, no Entry, name scanned once; (2) fill_erms/internal_move caller found:
every list() built a fresh DirStream (alloc + clear of the first FIRST_FILL bytes): list_reusing keeps one buffer per walk;
(3) walk_in no longer copies each name into namebuf nor rescans it (from_nul_free_terminated on the listing's own name; the
kernel name is NUL-free by construction in next_named).
Counts: see the percall job perf-nftw-entry-2-percall (nftw_entry.c, 41 entries/call) in ~/logs on keke; the number is NOT
in this note (jobs were still running at hand-back). fts_physical_1000 NOT measured (no bench job within budget).
Not to redo: Stat reuse, the three items above. Remaining trace items from run 1: set_path (2440 per 41 entries), Levels::fd
(1280), run closure (1403), alloc/dealloc per directory (Listing bytes/items could be reused across directories), Frame moves.
Next step: read the percall row; if above 1.05x of glibc, reuse Listing storage per level and make Levels::fd O(1) for the top level,
then record the remainder in docs/saturation/ (not yet done).
Jobs (keke): perf-nftw-entry-2-gate, -plants, -percall. Plants: dots-kept, name-short, pos-stuck; spare-buffer drop is equivalent (cost only).
Note: one edit of walk.rs was made with a python replace script instead of the Edit tool (rule slip, content verified by clippy).
