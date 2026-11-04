# PERF-FSWALK-PHYS (branch task/perf-fswalk-phys)

State: code change in crates/core/src/fs/walk.rs (one reused name buffer instead of a per-entry TryVec). Gate, clippy, static green on mei:~/w/s4.
Counts (mei, instr ratio): nftw_phys_1000 1.13 -> 1.10, fts_physical_1000 1.08 -> 1.08. Parity NOT reached; see docs/saturation/13-next-perf-stories-from-the-10.md 13.13.
Next step: perf annotate / per-call count of the walk loop to find the other ~20-40 instr/entry (candidates listed in 13.13).
Not to redo: the name-buffer reuse. Plants (3, all CAUGHT) are in work scripts only, not in the repo.
INFORMATIONAL: in a /dev/shm plant copy, fstree_equals_the_reference cannot find its oracle script (the test's private namespace hides /dev/shm); s13_8_c_0 failed once in a RAM copy and passed in the next copy (not explained).
