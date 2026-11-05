# PERF-STDIO-CHAR-FAST (branch task/perf-stdio-char-fast)

State: no product change kept. The one trim tried (low-byte alignment test in `tb::file_words`) was reverted
(commit c6c8772c on top of 9a2257c5); net code diff against next is empty.

## Finding: the brief's rows are stale
The brief's instr/op (getc_unlocked 43.3, putc_unlocked 40.2, getc 50.4, putc 47.2) predate the trusted-window
fast path (GETC-PUTC-FASTPATH, owner #1654, D-12.19), which is in next. Counted on this tree (kanon, Broadwell-EP,
job `perf-stdio-char-fast-counts-base`, `cargo xtask bench --compare --counters --runs 3 cbench_hot`, rc 0):

| row | instr Liella | instr glibc | ratio | time Liella/glibc dynamic |
|---|---:|---:|---:|---:|
| getc | 27.3 | 27.3 | 1.00 | 0.89 |
| getc_unlocked | 20.3 | 15.3 | 1.33 | 1.01 |
| putc_unlocked | 22.2 | 19.1 | 1.16 | 1.00 |
| putc | not in cbench_hot | | | |

Disassembly (release libc.so.6): getc_unlocked hit path is 13 instructions: null test (2), alignment copy+mask+branch (3),
two window loads, compare+branch, byte load, inc, store, ret. putc_unlocked is 14. Bench loop and indirect call account
for about 7 of the counted 20. glibc's own body is about 8, so the gap is the 5 instructions of null and alignment
checks on the `FILE *` plus one compare more.

## Tried
`addr % 8 != 0` -> `addr.to_ne_bytes()[0] & 7 != 0`: LLVM still emits `mov %edi,%eax; and $7,%eax`
(identical code), so no gain; reverted. Counts/plants for it were stopped (jobs killed).

## Not done / why
Reaching 1.05x (<= 16.1 getc_unlocked, <= 20.1 putc_unlocked) needs the null and alignment test of `fp` dropped from
the hit path (4 to 5 instructions). That reads an unchecked `FILE *` (UB for the caller, as in glibc, a fault)
and is the HELD category of the D-12.19 validation floor in docs/saturation/13 (trims a to c). RULING NEEDED, below.

## Next step
Owner ruling on dropping the `fp == 0 || fp % 8 != 0` test from `getc_c`/`putc_c`' trusted-window path (a NULL or
misaligned `FILE *` then faults, as in glibc, instead of returning EOF/EBADF). If yes: move the test into the
cold branch (window loads first would fault; so it needs a cheaper form, e.g. a signal-safe handler, which is not
cheap either). Recommend: keep as is; rows are at 1.00 to 1.33 with times at 0.89 to 1.01 (not slower than glibc).
