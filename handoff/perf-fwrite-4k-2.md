# PERF-FWRITE-4K-2 (branch task/perf-fwrite-4k-2) - NOT READY

State: two commits on 7bb2ba6c: write_straddle in crates/legacy/src/stdio_compat.rs (write_raw), a direct path for a block that
straddles the end of a full byte buffer on a descriptor (copy head, write buffer, keep tail), no run()/load/store/engine loop.
Clippy and `cargo xtask static` clean on the final head.

Per call (instructions:u, fwrite4k.c): before / after / glibc
- fwrite_4k 815.0 / 710.8 / 644.0 (1.27 -> 1.10; goal 1.05 not reached)
- fwrite_4k_win 592.5 / 579.5 / 558.3 (1.04)
- fwrite_4k_thru 507.0 / 528.0 / 455.0 (1.11 -> 1.16): SLOWER by 21, violates "no row slower". Cause not found: write_raw's
  early-out for blocks >= buffer is now two compares before write_direct; likely layout/register effect of the inlined
  write_raw. Next: move the straddle test into write_direct's None path or after `run`'s caller, re-measure thru.
- memcpy_4k_ref 440 / 440 / 425.
Trace (wl_trace, before): window call 549 (392 copy, fwrite_slow 105); straddle call 1010 (380 copy; State::write 150,
write_raw 105, fwrite_general 62, fwrite_slow 58, drain_to 53, store 45, Live::write 43, load 33).
Remaining gap: window call fwrite_slow 105 (lock + window), write wrapper 53.

NOT DONE: plants (no test of the straddle path exists/added; need a test of head/tail bytes, FILE words, partial write and
write error vs the engine, then plants on write_straddle conditions), the thru regression, bench --compare --counters, spec
parity1105.c is not in the tree (used fwrite4k.c). Gate (perf-fwrite-4k-2-gate2) was started on the final head; result not read.
Process note: I edited with a python rewrite once instead of Edit.
