# CLASSED-SLOT-INFRA handoff (one page)

**State:** branch `task/classed-slot-infra-5` (continues -4/-3/-2/-1; base origin/next, merge-base `41b8737b`). Product
code unchanged since `7ffdb9b3`; this run changed `bench/c/cbench_startup.c` (69b133a2) and the plants file (422be02f).
Role kanon:~/w/kn1 (Broadwell E5-2699 v4, AVX2 class).
**Verdict: INCOMPLETE (RULING NEEDED).** The no-swap classes are settled (no kernel gap, it was the bench's own
.bss). On the N100 the AVX2 class's swap costs 22-26 us per Liella process start, and the memset rows show no gain.

**Decisions in force:** D-4.53 (classed entries, M6), COMPAT-20 (`liella.compat.no_classed_swap=1`), rulings
#3138-#3141 (#3139: AVX2 class about 4 system calls and +2 VMAs per process, none on SSE2), #3297.

**Run -5 (this run):**
* **-4's gate and plants (shiki):** `classed-slot-infra-4-r2-gate` rc 0 (11314 passed, 0 failed); `-r2-plants` rc 0,
  8 CAUGHT, 1 WARN-NO-MSG (start-sse2-always). That plant failed for the right reason: the program's stderr says
  "expected the avx2 page at memset". But `xtask affected` names a failed conformance program and never prints its
  stderr. Fix 422be02f: the plant's cmd prints that file on failure. Rerun alone (kanon `classed-slot-infra-5-plants`):
  CAUGHT with the message.
* **The +5k kernel on no-swap classes was real, and it was the bench.** Interleaved `perf stat -r 50`, 12 rounds,
  order flipped each round (kanon `classed-slot-infra-5-perfmix`, load 2.5): COMPAT-20 +5395 kernel instructions
  (round sd 2479, se 716), SSE2 class (-AVX2) +4934 (sd 2397, se 692), each with exactly +1 minor fault. `perf record
  -e minor-faults -d` (`-faults`): the extra fault is a write from `cpu::init_with` to .bss page 0x7a1000. -4's
  `static char tunables_var[4096]` in cbench_startup.c (the measured child IS that program) sits first in .bss and
  pushed start-up's .bss onto an untouched page. Fix 69b133a2: pass the parent's own `GLIBC_TUNABLES=` entry (no
  buffer). After the fix (`-r2`, same method, load 23): COMPAT-20 -6398 (se 461), SSE2 class -6076 (se 817), each
  with 1-2 fewer faults than the merge-base (layout). Exact user instructions (single-step tracer): SSE2 class
  5345 -> 5353 (+8), COMPAT-20 5622 -> 5826 (+204, the flag's parse in key_of/signed_number). perf's user count
  reads +59 on the SSE2 class. Not chased: the exact tracer is the reference.
* **AVX2 class (default) on kanon:** kernel 195.5k -> 251.6k (+56k, se 0.5k), user +168, faults -1.
* **N100 timing** (nozomi `classed-slot-infra-5-n100-r2`, booked, load <1, 3 rounds, order alternating, medians,
  ns/op): exec_true_static 165180 -> 188892 (+14.4%), exec_true_echo 234989 -> 260970 (+11.1%), exec_hello_locale
  355505 -> 377225 (+6.1%). spawn_bin_true is unchanged (it spawns /bin/true, which is not Liella). memset_16/16_mix/64/64_mix/128
  are identical on both trees (1.89/17.04/2.10/17.04/3.35). Run 1 (`-n100`) agreed: startup +22.2 us. Its branch
  memset step failed because a `--staged` bench run rewrites `target/liella/bench/bench_times.tsv`, a staged
  product, so the next run refuses the tree. run2.sh unpacks the tarball again before every call.

**RULING NEEDED:** the swap meets #3139's count (4 calls, +2 VMAs), but in time it is +22-26 us per start on the N100
(+14% on a static `true` exec) and +56k kernel instructions on Broadwell, with no memset gain at 16-128 B.
Recommendation: do not enable the swap by default until (a) the guard/mremap split of that cost is measured
(`perf trace` printed nothing for the claude user; use a probe that times each of the 4 calls with clock_gettime)
and cut, and/or (b) an AVX2-page body is measured faster by enough to repay it in real workloads.

**Exact next step:** get the ruling. If the swap stays, write the per-call time probe (openat pagemap, pread, close,
mremap, each timed 1000x in a fresh process) on the N100 and Broadwell, and shrink the largest part. If it goes
default-off, invert COMPAT-20's sense (a new opt-in tunable) and re-time.

**Not to redo:** no-swap class costs (settled above); the fixed user cost (-4); syscall/VMA bound (-3); memset
timing on both classes. Scripts: kanon `~/work/classed-slot-infra-5/{perfmix.sh,faults.sh,bss.sh,r2.sh,step.c}`,
nozomi `~/work/classed-slot-infra-5/run2.sh`; merge-base tree kanon `/mnt/zram/claude/classed-slot-infra-5/mb`.
Analyzer: chisato `/home/claude/w/.scratch/CLASSED-SLOT-INFRA-5/analyze.py`.

**Read:** docs/design/loadtime-entry.md 5.3-5.5; D-4.53; handoff/classed-slot-infra-plants.txt.
