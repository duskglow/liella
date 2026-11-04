# PERF-GETC-PUTC-TRIMS (register 13.2 row 9)

State: branch task/perf-getc-putc-trims. Trim (d) only (coordinator #1654); (a) to (c) held for the owner ruling.

Change: crates/boundary/src/stdio.rs `getc_c`: the General miss is a separate cold tail call
`getc_slow_general`, so LLVM no longer hoists the shared ReadFront argument (`mov $2,%al`) to the entry.
Disassembly (keke r1, release): `fgetc_unlocked` hit path 34 to 33 instructions. putc: nothing to move
(its slow-path arguments were already after the hit path).

Counters (kanon not used; keke role r1, `cargo xtask bench --compare --counters --runs 5 cbench_hot cbench_stdio`, default level):
getc_unlocked 43.4 instr vs glibc 15.3 (i 2.82; before 44.3 / 2.89); putc_unlocked 40.2 vs 19.1 (i 2.10, unchanged).
The -AVX2 run is in job perf-getc-putc-trims-counts (log ~/logs/perf-getc-putc-trims-counts.log on keke).
No timing was taken (counter story; timing belongs to the release A/B).

Gate: direct tier green (perf-getc-putc-trims-gate, rc 0). Plants (perf-getc-putc-trims-plants, rc 0): general-eof,
general-locked, empty-eof CAUGHT (msg: sym_*getc*); general-as-empty EQUIVALENT (getc_refill re-checks and falls to general).
clippy workspace, xtask clippy, xtask static: pass.

RULING NEEDED: (a) drop the `_mode` test, (c) drop the buffer-null test, (b) one bound via get_unchecked. Recommendation: hold;
about 1 to 3 instructions each, together at most about 2.5 of about 13.6 cycles per getc.
Do not redo: (d).
