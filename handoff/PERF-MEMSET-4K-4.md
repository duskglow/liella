# PERF-MEMSET-4K-4 (branch task/perf-memset-4k-4, stacked on task/perf-memset-4k-3)

State: one code commit on top of -3's head 73ac4f81: 92265e31 "Run memset's rep stosb from d when d is 64-byte
aligned". Handoff of -3: handoff/PERF-MEMSET-4K-3.md (still the record of the entry's kernel choice). Role: keke:~/w/n2
(Zen 1, AVX2, no ERMS).

## Step 1: the -3 red (procedural)
-3's plants label was run twice; the second run was refused (its directory existed) and overwrote the log. Cause:
reused label + reused dir. Fixed by running the plants of THIS head under a new label and a new RAM dir
(perf-memset-4k-4-plants, /dev/shm/claude/perf-memset-4k-4/plants-1), through `cargo xtask plants` (log has
`plants: copy`). -3's three head-path plants (handoff/PERF-MEMSET-4K-3-plants.txt) target code this commit does not
change and were not rerun.

## The change
`.Lset_erms` (csys/src/machine/fold.S): `testb $63, %dil; jnz .Lset_erms_head` first. A 64-byte-aligned d (NULL
included) then takes the NULL test and `rep stosb` over all n from d (movq rdx->rcx, movl esi->eax, rep stosb,
rax = rdi - rdx, ret). An unaligned d is never NULL, so the alignment test replaces the NULL test on the head path:
that path's count is unchanged. Time: for an aligned d the old code started `rep stosb` at the 64-byte boundary
d + 64 after storing d..d+64 by hand; the new one starts it at the boundary d. Same alignment, same bytes, no
timing risk. Unaligned d: identical instructions.

## Profile (instructions:u per call, spec validation/percall/parity1105.c case memset_4k, harness loop included)
- ERMS path (Haswell-EP class, threshold 1280), counted from the code, NOT MEASURED (no ERMS host in this run; the
  role is Zen 1 and the tunables cannot turn on ERMS the CPU lacks). Entry to `.Lset_out`: mov, 3 compare/branch
  pairs (10), word compare (2), then before: NULL test (2) + broadcast (4) + head stores (4) + alignment (5) + rep +
  mov + ret (3) = 28 own + ~13 harness = 41 (-3 measured 41 on shiki; glibc 30).
  After: aligned d 10 + 2 + 2 (align) + 2 (NULL) + 6 = 22 own -> ~33 (1.10); unaligned d unchanged 41 (1.37).
  The row's buffer is `static char m1105[4096]`: whether it is 64-byte aligned depends on the link layout (not checked).
- Zen 1 (keke, no ERMS: the AVX2 fill), MEASURED, job perf-memset-4k-4-percall: glibc 250, new 238 (0.95). Trace
  (perf-memset-4k-4-trace, call 11): 225 in memset = entry 17 + `liella_machine_fill_avx2` 208 (31 128-byte steps x 6
  + 22 setup/ends). Zen is already below the reference; this commit does not touch that path.

## First-principles bound (ERMS path, unaligned d)
Keeping a 64-byte-aligned `rep stosb` start (a timing choice), the path needs the entry chain to the >256 class (10:
fold limit and the 65-128 / 129-256 class tests, each needed by a smaller row), the word compare (2), the alignment
test (2), broadcast (4), head stores (4), the boundary arithmetic (5), rep, restore, ret (3) = 30 own. Rejected
reorderings: testing the word before the 65-256 classes (+2 on memset_128/256, -3 handoff). The 1.05 target at
unaligned d is reachable only by starting `rep stosb` at d (-10: about 31, 1.03), which changes timing on ERMS hosts:
needs a timing booking on an Intel ERMS host (unaligned vs aligned start at 4 KiB and 64 KiB) before it is tried.

## Jobs (keke)
- lint: perf-memset-4k-4-lint (clippy --workspace --all-targets --all-features -D warnings + xtask static): rc 0.
- gate: perf-memset-4k-4-gate (`cargo xtask affected --run`): 10 suites ok, 10199 passed, 0 failed; conformance 117/0;
  peak 12G; 2 SKIPPED lines (tool or capability absent, not from this change).
- plants: perf-memset-4k-4-plants (handoff/PERF-MEMSET-4K-4-plants.txt): 4 CAUGHT, 1 EQUIVALENT (alignment choice
  swapped: both branches fill correctly), 0 SURVIVED, 0 WARN.
- counts: perf-memset-4k-4-percall, perf-memset-4k-4-trace.
- Superseded: perf-memset-4k-4-percall-before and -trace (first attempts, killed while still queued: they held the
  role and blocked the sync of the new head; never ran).
- Tests: small_copy's NULL test gains a run with `rep stosb` forced on (sizes 1280, 4096, 65536), so the entry's ERMS
  branch NULL test is checked on hosts without ERMS (plant ms4-null proves it).

## Next step
1. Count memset_4k on an ERMS host (Haswell-EP / Skylake-SP) to confirm 33 aligned / 41 unaligned, and print
   `&m1105 % 64` in the probe to know which the row measures.
2. With a timing booking on an Intel ERMS host: time `rep stosb` from an unaligned d against the aligned start at
   4 KiB and 64 KiB. If no loss, drop the head on the ERMS path (about 31, 1.03); if a loss, record the bound above
   as the saturation row for memset_4k.
