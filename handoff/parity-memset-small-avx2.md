# PARITY-MEMSET-SMALL-AVX2 (branch task/parity-memset-small-avx2, base origin/next f2157504)

State: CHECKPOINT at the context budget (139k/140k) after design analysis only. No code change, no jobs run.
Role: shiki:~/w/sh1.

## Per-phase split (counted from csys/src/machine/fold.S `liella_machine_memset`, own instructions, NOT MEASURED)
Nodes = cmp+jcc pairs. Leaf = broadcast (SSE2 4: movd/punpcklbw/pshuflw/punpcklqdq) + stores + ret.
| class | path | own | rows |
|---|---|---:|---|
| 16-32 | mov, fold node, 32 node, 16 node, leaf 7 | 14 | memset_16 |
| 33-64 | mov, fold node, 32 node, leaf 9 | 15 | memset_64 (AVX2 class 20.3 vs 18.3: needs -2) |
| 65-128 | mov, fold node, lea+63 node (3), NULL 2, leaf 13 | 21 | memset_128 (27.4 vs 24.4: needs -2) |
| 129-256 | mov, fold, lea+63, 191, NULL, avx2 flag, leaf (SSE2 21 / AVX2 12) | 33 / 24 | memset_144/256 |
| >256 ERMS | mov, fold, lea+63, 191, erms node | 10 to .Lset_erms | memset_4k/64k (rep rows) |
The surplus on the AVX2 class is the SSE2 leaf (broadcast 4 + 16-byte stores) used for 16-128 on AVX2 hosts.
AVX2 leaves would save 2 (16-31), 3 (33-64) and 5 (65-128).

## Finding: the SSE2 tree is already at its minimum, so the AVX2 leaves need a host split
- SSE2 broadcast of an arbitrary int's low byte takes at least 4 instructions (imul needs movzbl first; pshufb is SSSE3). 33-64 needs depth 2 (interior range).
- Every leaf is shared code, so an AVX2-only leaf needs a node where the hosts diverge. On a cmp/jcc tree that node always adds 2 to some SSE2 row (Ivy Bridge) or to an AVX2 row. I worked through the alternatives: host-specific fold-word thresholds (AVX2 F=32), class words, the flag byte, bsr tables. Each one either raises an Ivy row (memset_128 +1/+2, 129-256 +2, >256 +2) or leaves AVX2 33-64 at 15.
- Cheapest host split: one `jmp *ptr(%rip)` at `.Lset_big`, with three subtrees (SSE2-fold, AVX2-fold with F=32, fold-off = today's code). Predicted own counts (NOT MEASURED):
  SSE2: 65-128 21 (=), 129-256 31 (-2), >256 8 (=), <=64 unchanged.
  AVX2 subtree with the 128 split first: 33-64 14 (-1, ~1.055 on memset_64), 65-128 18 (-3, ~1.00), 129-256 22 (-2), >256 8 (=).
  With the 64 split first: 33-64 12, 65-128 18, 129-256 24, but the >256 ERMS path is +2 (rep rows, so it would be judged in cycles).
- Cost of that design: a writable code pointer in .bss that the hot entry jumps through. That is a hardening (control-flow hijack) surface, and Liella has no precedent for it (no `jmp *mem` dispatch, no IBT). It needs a ruling.
- The alternative with no pointer is IFUNC-style per-host `memset` (IRELATIVE slot in RELRO; the code has IRELATIVE references in ffi.rs/runtime.rs, D-4.25 "MEM-IFUNC" chose an AtomicBool). It makes the host split free but is larger than an S story.
- Small landable piece (no ruling): a word H (128 with folding on, 0 off) replacing `lea -65; cmp $63; ja` with `cmpq H(%rip),%rdx; ja .Lset_129`, with `.Lset_129` using `lea -129; cmp $127`. That is -1 on 65-128 on both classes (Haswell memset_128 34->33, 1.03; Skylake-SP 27.4->26.4, 1.08), with 129-256 and >256 unchanged. Fold-off 65-128 goes to Rust (`liella_hot_fill_rest`; check it does not re-enter the machine entry). The word line in fold.S is full (64 bytes), so H needs its own aligned line.

## Next step
1. Ruling (below). 2. Either implement the jmp* design or the IFUNC route, or land the H-word piece alone. Profile first with wl_percall (avx2_families.c) on the Skylake-SP and Ivy hosts. 3. Plants via `cargo xtask plants`, direct gate, counts on both classes.

## RULING NEEDED
Host split for the memset entry: (a) a writable code pointer `jmp *` (1 instruction; hardening cost), (b) IFUNC/RELRO per-host entry (0 instructions; larger story), or (c) stop at the H-word piece. Recommend (b) as a follow-up story and (c) now.
