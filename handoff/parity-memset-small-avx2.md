# PARITY-MEMSET-SMALL-AVX2 (-2) (branch task/parity-memset-small-avx2-2, base origin/next f2157504)

State: the H-word change (ruling #3069 option (c)) is committed: fbd23bc4 (code), aa95519f (plants file), then this
handoff. Role keke:~/w/r1. Clippy and `cargo xtask static` clean on the role at fbd23bc4 (foreground).
Jobs on keke (started 17:40Z, queued behind load ~30): parity-memset-small-avx2-2-gate (direct tier),
parity-memset-small-avx2-2-plants (`cargo xtask plants handoff/PARITY-MEMSET-SMALL-AVX2-2-plants.txt --dir
/dev/shm/claude/PARITY-MEMSET-SMALL-AVX2-2/plants-1`), parity-memset-small-avx2-2-percall (wl_percall,
avx2_families.c, memset_16..4096, base f2157504). Results: see "Results" (filled when collected).

## Decisions in force
- Ruling #3069 (coordinator): (c) only. (a) writable code pointer `jmp *`: rejected (redirect target for a write bug).
  (b) IFUNC/RELRO per-class entry chosen once at load: goes to the owner as a dispatch-mechanism change; not started.

## The change (fbd23bc4)
- `liella_machine_set_mid` (fold.S, .data, shares the 64-byte line of `liella_machine_fill_erms_from`): 128 while NULL
  folding is on, 0 off. `.Lset_big`: `cmpq set_mid(%rip),%rdx; ja .Lset_129` replaces `lea -65; cmp $63; ja`.
  `.Lset_129`: `lea -129; cmp $127; ja .Lset_out`. Own instructions: 65-128 -1 (both classes); 129-256 and >256 unchanged
  (the subtraction moved); <=64 unchanged.
- Writer: `dispatch::select_null_fold` stores 0 before the fold limit drops and 128 after it rises;
  `unresolve_null_fold` stores 0 before the fold limit. Invariant: never nonzero while the fold limit is 0 (1-64 would
  take the 65-128 leaf).
- Folding off: 65-128 now goes to `liella_hot_fill_rest` -> `fill_small` -> `fill_blocks::<4>` in Rust. Checked: that
  path does not call `liella_machine_memset` (no recursion), and `resolve_fold(n)` does nothing for n > 64, so the
  "a size above 64 must not read mmap_min_addr" test still holds. Folding is on in production on every host with
  vm.mmap_min_addr >= 64 KiB (after the first 1-64 call); fold-off 65-128 is slower by the Rust path (not a measured row).

## Plants (handoff/PARITY-MEMSET-SMALL-AVX2-2-plants.txt; tests: small_copy fills/folding-decision)
psm-word (word 192), psm-off (word 128 with folding off), psm-unresolve (unresolve keeps the word), psm-129-bound
(`cmp $128`), psm-129-base (`lea -130`), psm-jump (`ja` -> `jb`).

## Results
(pending: gate, plants, percall; see the hand-back / report json)

## Not to redo
The paper analysis (previous run) and the host-split options; the Rust fold-off 65-128 recursion check (done above).

## Next step
1. Collect the three jobs. Percall rule: any memset row higher than base on this host (Zen 1, AVX2 class) -> revert
   fbd23bc4 and hand back DO NOT LAND. The Ivy Bridge (SSE2) class was not counted in this run: the change has no
   class-specific code (same instructions on both builds), so its counts follow; the timing sweep judges both.
2. Follow-up (owner ruling needed first): (b) per-class `memset` entry chosen once at load (RELRO), for the AVX2 leaves
   (predicted, not measured: 33-64 -1, 65-128 -3, 129-256 -2 on the AVX2 class).
