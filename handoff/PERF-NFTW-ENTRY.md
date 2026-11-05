# PERF-NFTW-ENTRY (branch task/perf-nftw-entry, role sh2)

State: partial. Per entry nftw is 694 instr (was 772; reference 212) in the new percall spec validation/percall/nftw_entry.c (38 files, FTW_PHYS). Parity (1.05x) NOT reached: 3.28x now vs 3.64x before on that shape.
Done: walk stats every entry into one reused Stat (new Sys::fstatat_into) instead of a zeroed 144-byte Stat moved through Result<(Kind,Option<Stat>)>: -78 instr/entry.
Trace (wl_trace nftw, 32.5k instr per 41 entries), instr per call: list 6058, walk_in 4733, fill_erms 4107, internal_move 2605, set_path 2440, run closure 1403, KStr::from_nul_terminated 1390, extend_from_slice 1360, Levels::fd 1280, examine 1280, Listing::get 880, alloc/dealloc 1330.
Next step: (1) list(): per dirent DirStream::next builds an Entry, name_z copies, Item pushed: use a buffered_addr-style record walk; (2) fill_erms/internal_move (6.7k) are likely per call (directory buffer clear and moves): find the caller; (3) set_path + from_nul_terminated + extend (about 130/entry): stat by the listing's terminated name without rescanning.
Not to redo: the Stat reuse. Jobs (host shiki, run serially by one jobrun chain): perf-nftw-entry-gate (clippy, static, affected), -plants (3 plants), -counts (bench fstree, after). Before-counts are the brief's (nftw_phys_1000 1.10, fts_physical_1000 1.08).
NOT VERIFIED: fts row (not measured), bench after-counts, gate, plants: jobs may still be running.
