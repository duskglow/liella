# PARITY-MEMSET-SMALL-AVX2 (-3) (branch task/parity-memset-small-avx2-3, origin/next 41b8737b merged in)

## -3 (gate rerun only, coordinator note 11-05)
- Merged origin/next 41b8737b (cc738ac7; carries ENOMEM-REALPATH-INTERMITTENT-2's measure guard and -3's heap-stats
  test). No code change of this story's own. next touched fold.S (comments only) and ffi.rs (copy contract docs); the
  planted files' code and crates/boundary/tests/small_copy.rs are unchanged by the merge.
- Role mei:~/w/s1 (Skylake-SP). clippy --workspace --all-targets --all-features -D warnings and `cargo xtask static`
  clean at cc738ac7 (foreground).
- Gate parity-memset-small-avx2-3-gate (direct tier, --base origin/next) on mei at cc738ac7: rc 0. Every suite ok:
  liella 435.8 s, liella (exports interposed) 418.9 s (enomem_fs_walk was selected and passed: the -2 failure did not
  recur on the merged head), liella-abi, liella-boundary (+exports), liella-legacy, liella-math (+exports), liella-rt
  (+exports), conformance 117/117, fuzz-check. Peak 15G, oom_kill 0. 2 SKIPPED lines (skip_note_visible,
  skip_precondition_fails: the harness's own skip tests).
- -2's enomem_fs_walk red: not reproduced here; not shown caused by this change (the memset change touches no
  allocation path; fold-off 65-128 runs Rust fill_small). The intermittent stays ENOMEM-REALPATH-INTERMITTENT's
  (cause unproven per its -3 handoff). No base-vs-head rerun was needed (brief: only if it still failed).
- Plants rerun on the merged head (same file): parity-memset-small-avx2-3-plants rc 0, dir
  /mnt/zram/claude/PARITY-MEMSET-SMALL-AVX2-3/plants-1: 5 CAUGHT, 0 SURVIVED, psm-unresolve WARN-NO-MSG (same
  understood cause as -2: panic "call 117901063: the first small call must decide" at small_copy.rs:380).
- percall: not rerun (memset code identical to fbd23bc4; -2's keke count stands: memset_65/128 34 -> 33, no row up).
- Next step: land. Follow-up (b) per-class entry is with the owner (LOADTIME-ENTRY-DESIGN rulings #3108-#3110).

## Record of -2
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

## Results (keke, code fbd23bc4)
- percall (wl_percall avx2_families.c, base f2157504; glibc / base / new): memset_65 30/34/33, memset_128 30/34/33;
  memset_16 24/27/27, 31 24/27/27, 32 25/27/27, 33 25/27/27, 64 25/27/27, 129 37/37/37, 256 37/37/37,
  1024 82/94/94, 4096 250/238/238. No row rose.
- plants (plants-1): 5 CAUGHT, psm-unresolve WARN-NO-MSG: it failed at the expected assert (line 380), but the 1-byte
  memset going through the 65-128 leaf wrote 0x07 bytes over the loop variable, so the message read
  "call 117901063" (0x07070707) instead of "call 2". The cause is understood and the failure is deterministic.
- gate (direct): RED. liella (exports interposed) 1573 passed, 2 failed: enomem_fs_walk enomem_realpath_null
  ("realpath: allocation 2: a partial result stays allocated (5808 -> 6832)") and enomem_scandir_filter_alphasort
  (same assert, enomem_fs_walk.rs:412). No other log on keke shows this message, so it is NOT shown to be unrelated.
  Cause not found (budget). Hypothesis to test first: run the test at base f2157504 and at the head.

## Not to redo
The paper analysis (previous run) and the host-split options; the Rust fold-off 65-128 recursion check (done above).

## Next step
1. Collect the three jobs. Percall rule: any memset row higher than base on this host (Zen 1, AVX2 class) -> revert
   fbd23bc4 and hand back DO NOT LAND. The Ivy Bridge (SSE2) class was not counted in this run: the change has no
   class-specific code (same instructions on both builds), so its counts follow; the timing sweep judges both.
2. Follow-up (owner ruling needed first): (b) per-class `memset` entry chosen once at load (RELRO), for the AVX2 leaves
   (predicted, not measured: 33-64 -1, 65-128 -3, 129-256 -2 on the AVX2 class).
