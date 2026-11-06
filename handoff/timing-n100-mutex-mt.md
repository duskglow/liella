# TIMING-N100-MUTEX-MT handoff (INCOMPLETE: do not land as it stands)

State: branch task/timing-n100-mutex-mt, code 815af0ca, tests 26ab6e90 (HEAD = the commit that adds this file).
Base origin/next 730f416b. Role keke:~/w/n3.

## What was found (step 1 of the brief)
Annotated the N100 sweep profile (rec.mutex_lock_unlock_normal_mt.data + the staged cbench_hot, disassembled on a build
host): 90.7k of ~175k cycle samples sit on the instruction after unlock's `xchg`, and 39k after lock's `lock cmpxchg`.
Locked instructions per pair: exactly one in lock (`lock cmpxchg`, Acquire) and one in unlock (`xchg`, Release); no fence,
no SeqCst RMW. So the protocol already has the minimum. The difference was what sat ahead of each one: none ahead of the
cmpxchg; ahead of the xchg a `push %rax` (the frame LLVM placed at the entry because the export had two call sites, the
inline futex wake and the cold slow path) plus the owner and count stores (and lock's owner/count stores still in the
store buffer).

Probe (black-box, keke, `~/bin/probe`, hash b2d4f23094db): glibc leaves `__count` 0 for normal, error-checking and
adaptive mutexes, held or free, before and after pthread_create; 1 for a recursive one held once. Liella stored 1/0 for
every type. glibc also sets `__nusers` 1 while held; Liella leaves it 0 (existing difference, not changed, INFORMATIONAL).

## The change (815af0ca)
- `__count` is kept by recursive mutexes only (lock_fast, unlock_fast_deferred, and the general lock path for the private
  plain types; robust/PI/PP/pshared unchanged).
- `Mutex::unlock_fast_deferred` returns Done/Wake/Slow; the export does the wake through the single cold call
  (`mutex_unlock_slow(.., wake)`), so the fast path has no frame. No ordering changed (no D-row needed for ordering; a
  D-row for the `__count` layout is still to write, see next step).
- Tests (26ab6e90): `only_a_recursive_mutex_keeps_a_count`, and a count-word assertion in `every_mutex_type_excludes`
  (covers the contended general path).

## Results
- Gate TIMING-N100-MUTEX-MT-gate (keke, on 26ab6e90): rc 0; clippy and static clean; 6 suites, 852 passed / 0 failed /
  92 ignored; conformance 16 passed / 0 failed.
- Counts (keke, Zen 1 AVX2; instr per call Liella / glibc), before -> after:
  normal 64.0 -> 64.0; normal_mt 63.2 -> 63.1; recursive 83.1 -> 86.0 (+2.9); recursive_mt 82.2 -> 85.2 (+3.0).
  The recursive rows FAIL the brief's "no row gains instructions" gate.
- N100 timing (nozomi, booking 51, ~/bin/timing label TIMING-N100-MUTEX-MT-n100b, 3 interleaved rounds, staged trees;
  ratio Liella/glibc, stable across rounds): normal_mt 1.215 -> 1.013 (28.06 -> 23.40 ns); normal 0.904 -> 0.761
  (7.95 -> 6.69 ns); recursive_mt 0.848 -> 0.940 (24.87 -> 27.62 ns, SLOWER).
  Note: both stage manifests say commit 815af0ca; the before tree was the origin/next versions of the two files checked
  out over it (tree hashes 40734fe8 before, e2d3067d after).
- Plants TIMING-N100-MUTEX-MT-plants (keke, `cargo xtask plants`, 6 plants) still running at hand-back; rt-wake-dropped
  CAUGHT (by timeout). WARN-DEV-SHM printed (dir under /dev/shm, as developer-flow says for Zen 1).
- Ivy Bridge timing: not run (budget).

## Why recursive regressed
The new `if k == MUTEX_RECURSIVE { count.store(1) }` after the CAS keeps `k` live; LLVM then folded lock's entry tests
into setcc/test chains (`test ecx; setg; cmp $4; setb; test r8b,sil`), the pattern MUTEX-5 had removed, and laid out the
recursive store as the fall-through: +2 in the prologue, +1 after the CAS. Unlock lost its push/pop.

## Exact next step
Fix lock_fast's codegen so recursive does not gain: e.g. split lock_fast by type before the CAS (normal/errorcheck/
adaptive path without the count store; recursive path with it), or keep the entry tests as separate branches (check the
disassembly of `__liella_libc___pthread_mutex_lock` in target/liella/libc.a; disassemble one member, the full archive
takes minutes). Then: counts (no row up), N100 timing of the 3 rows + Ivy Bridge check, plants rerun, D-row for the
`__count` layout (probe-settled, matches glibc) in plan/07-decisions/, saturation note. If no layout keeps recursive
level, the push removal alone (no count change) is the fallback to measure.

## Do not redo
The annotation (above); the probe (cached, hash b2d4f23094db); staged trees on nozomi ~/work/TIMING-N100-MUTEX-MT/{before,after}.
