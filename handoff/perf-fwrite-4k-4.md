# PERF-FWRITE-4K-4 (continues PERF-FWRITE-4K-2/-3; branch task/perf-fwrite-4k-4) - READY

State: write_straddle (crates/legacy/src/stdio_compat.rs) is reached from write_direct's Full arm; the write-through
row no longer pays for it: window_put was trimmed on the shared path (found by disassembling write_raw on mei, no guessing).
Changes in -4: window_put looks up the write-pointer word once (one length check), tests role after it, ends the room
test with one `at + len > min(wend, buf_len)` comparison (a pointer before the buffer wraps to a huge offset), reads the mode
word only when the copy happens, tests `buf == 0` last; the straddle gate compares data.len() with buf_len (the callee keeps
the exact `len - RESERVE` test). Zero-length blocks outside the window go to the engine (same result).

Per call (instructions:u, validation/percall/fwrite4k.c, Broadwell-EP class host; glibc / base / final):
- fwrite_4k 645.0 / 798.7 / 683.5 (1.06; the -3 goal of 1.05 not quite reached)
- fwrite_4k_win 559.3 / 562.0 / 547.6 (0.98)
- fwrite_4k_thru 455.5 / 507.0 / 504.0 (no row slower than base)
- memcpy_4k_ref (glibc 426, ours 83: the harness row is a different copy path, unchanged)

Tests: crates/core/tests/stdio_straddle.rs (4 tests; clause IDs named). Plants (all 7 CAUGHT, job perf-fwrite-4k-4-plants4,
tests stdio_straddle + s12_stdio_clauses + stdio_short_io + stdio_products): drain-start, tail-ptr, err-count, err-flag,
pos-known, head-copy (write_straddle), window-ptr (window_put).
Gate perf-fwrite-4k-4-gate: static, clippy --workspace --all-targets --all-features -D warnings, affected --run (2642 passed, 0 failed).

Not to redo: the straddle path, its tests, the window_put trim. Open: the new test block_that_fits_a_large_buffer... does NOT
reach window_put through x::fwrite (the plant on window_put survives stdio_straddle alone; the s12_stdio_clauses and
stdio_short_io tests catch it), so it covers the boundary's own fast path; fine to keep or drop. Straddle gate plant:
`data.len() < buf_len` changed to never-true is EQUIVALENT by behaviour (slower only; the percall row, not a test, catches it).
Next step: none required; fwrite_4k to 1.05 would need the 53-vs-21 instruction wrapper chain (fwrite -> fwrite_slow -> fwrite_general).
Process notes: the entry of write_raw merged the length/role/buffer tests into setcc/and sequences; ordering the loads (word
lookup first) cut 3 instructions. Disassembly first would have saved the -3 run.
