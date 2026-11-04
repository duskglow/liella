# PERF-GETC-PUTC-TRIMS-2 handoff
State: branch task/perf-getc-putc-trims-2. Merge-only run. next already carries trim (d) in `getc_c`
(PERF-GETC-PUTC-2, register 12.10), so the earlier branch's stdio.rs change was dropped (no code diff vs origin/next);
only register/handoff text was taken. Register row 9 updated with the re-count.
Counts (shiki, `cargo xtask bench --compare --counters --runs 5 cbench_hot`, job perf-getc-putc-trims-2-counts2):
getc_unlocked 43.3 instr/call (i 2.83), putc_unlocked 40.2 (i 2.10): unchanged from the earlier branch's numbers.
Gate: perf-getc-putc-trims-2-gate rc 0 (docs-only change; nothing selected). Plants: none, no code touched.
Trims (a) to (c) still wait for the owner's safety-vs-speed review; do not build them.
Bench names are positional and are the cbench_* files (getc_unlocked is a row of cbench_hot, not a bench).
Next step: none for this story; (a)-(c) on the owner's ruling.
