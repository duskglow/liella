# PERF-REALLOC-USABLE-RECHECK (handoff)

State: branch task/perf-realloc-usable-recheck, docs-only change (saturation register row N3-4 re-measured). No code change.

Measured (mei, Broadwell-EP, validation/wl_percall.sh, job perf-realloc-usable-recheck-percall-before2, spec rz.c in the
role's scratch ~/work/PERF-REALLOC-USABLE-RECHECK/): in-place realloc of a held block 101.0 vs glibc 56.0 instr (loop
included); bench triple malloc/realloc/free 205 vs 190 (1.08); malloc+free 112 vs 142. No regression since 10-24; the gap is
about 45 instr per in-place realloc, not located.

Things not to redo: a `--disassemble=` of the alloc cgu object in libc.a (it holds only empty alias sections; 3 calls lost);
the first percall run failed because `cargo` was not on PATH over ssh (source ~/.cargo/env in the job).
Decisions in force: REALLOC-FUSE (Heap::resize fused in-place path). Open: which instructions of the exported realloc
beyond a free-like lookup can go (the liveness check cannot).
Next step: disassemble the real exported `realloc` from the linked libc.so.6 / a static percall binary (objdump -d on the
final binary, symbol realloc), count by region; then try dropping the wrapper work (ret::null/checked, DEBUG word) on the
in-place return. Then counts-before/after with `cargo xtask bench --compare --counters --runs 3 cbench_malloc`.
