# PERF-STDIO-UNLOCKED-TRIM (branch task/perf-stdio-unlocked-trim)

State: code commit 827f0ff2 (on top of 03c21c23). Ruling #2694 / owner #1654 applied: the `fp == 0 || fp % 8 != 0`
test is gone from the getc/putc front paths (`read_front_ext`, `putc_c`) via `tb::file_words_trusted`; a NULL or
misaligned FILE* now faults on the first window load. All slow paths and every other caller still use `tb::file_words`.
Valid-stream behaviour unchanged. No existing test asserted EOF/EINVAL for a NULL stream on getc/putc (none edited).

## Counts (shiki, Haswell-EP E5-2697 v3, `cargo xtask bench --compare --counters --runs 3 cbench_hot`)
| row | instr before | after | glibc | ratio before -> after |
|---|---:|---:|---:|---|
| getc | 27.3 | 22.3 | 27.3 | 1.00 -> 0.82 |
| getc_unlocked | 20.3 | 15.3 | 15.3 | 1.33 -> 1.00 |
| putc_unlocked | 22.2 | 17.2 | 19.1 | 1.16 -> 0.90 |
No row of the 58 in cbench_hot has more instructions after. putc (locked) is not in cbench_hot (not counted).
Jobs: perf-stdio-unlocked-trim-counts-before (base tree in RAM), -counts-after, -gate (clippy, static, affected --base
origin/next: rc 0), -plants3 (plants: re-add `tb::file_words(fp).is_none()` slow-path return on the getc / putc fast path;
CAUGHT when the unlocked row ratio > 1.05).

## Not to redo / next
Nothing remaining; targets met. SUPERSEDED jobs: plants, plants2 (killed at start: wrong exit logic / reused copy dir).
Plan rows: D-12.19, saturation 13 (HELD category trims a to c now taken by ruling #2694: the register row should be updated by the coordinator).
