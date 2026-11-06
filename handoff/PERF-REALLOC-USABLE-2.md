# PERF-REALLOC-USABLE-2 (NOT READY)
Branch task/perf-realloc-usable-2. Change: the `realloc`/`__libc_realloc` exports test the in-place fit first
(`Heap::fits_in_place`, a copy of the first test of `Heap::resize`) and return the same pointer; everything else goes to
the out-of-line `realloc_general`. Result: `realloc` of a held 17 B block to 24 B is 89 -> 74 instructions in one call
(wl_trace, shiki host, 3 push/pop pairs and the frame gone). glibc's in-place realloc is about 46-50, so the target (1.05x
on the bench triple) is NOT met: bench triple expected about 201 vs 195 (not measured; no --compare run was made).
Remaining waste seen in the disassembly: the class-lower-bound chain (`below/2`: clamp, saturating_sub, table load, shift,
about 8 instr; a per-class const table would drop it), the `K_REC` class bounds check with its panic path (keeps one push),
the debug and size-range checks. Next step: const lower-bound table, then `cargo xtask bench --compare --counters`.
Gate on the code head (before the test): green, 4826 passed. Plants (3, all CAUGHT): fits ignores class, ignores lower
bound, accepts size 0. Final gate (gate2) was RED on `opaque-audit` for the new test (direct export calls); the fix, an
`// opaque-ok:` line, is committed but NOT re-run (budget). Time ratio (1.33) not measured.
Counts: percall4 (trace, 74 instr) only; no before/after bench.
