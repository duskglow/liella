# PERF-REALLOC-USABLE-3 (READY, target 1.05x on the in-place call itself not met)
Branch task/perf-realloc-usable-3 (stacks on -2). Verified the -2 opaque-audit fix: static + clippy + gate green on the head.
Added: `HALF_BELOW` const table replaces the class lower-bound chain in `Heap::fits_in_place`; test now checks the bound one
request either side in two classes (the -2 test crossed it only by a wide margin: the quarter plant survived it).
Counts (keke, Broadwell-EP, wl_percall realloc_usable.c): malloc+realloc+free 181 vs glibc 192 (0.94); malloc_free_17 114 vs 144.
In-place realloc alone was 74 instr (-2 trace), glibc ~48; not re-traced after the table (about -6 expected, not measured).
Plants: zero and quarter CAUGHT; "self" (stricter bound) EQUIVALENT: it only sends the call to the general path, which keeps
the block by the same rule. Remaining gap recorded in saturation register 15, row 4.
Jobs (keke): perf-realloc-usable-3-gate2, -plants3, -percall2 (superseded: -gate, -percall on the pre-test head; -plants, -plants2).
Next: per-instruction profile of the linked export (objdump of libc.so.6 symbol realloc), then trim the record-bounds panic path.
