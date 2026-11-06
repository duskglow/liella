# CLASSED-SLOT-SWAP-COST handoff (one page)

**State:** branch `task/classed-slot-swap-cost` (stacks on CLASSED-SLOT-INFRA-7, `a8f38570`), code/docs commit
`2bcd8655`, then this handoff. Role keke:~/w/n2 (Zen 1, AVX2 class). No product behaviour changed; the swap stays OFF.
**Decisions in force:** D-4.53 (amended here: cost table, floor), COMPAT-20, rulings #3139, #3140, #3434.

**Result: the floor is above the target; COMPLETE with the floor proved (brief step 3, second branch).**

Probe: `~/work/classed-slot-swap-cost/probe.c` on keke, kanon, nozomi, nico (static, linked against Liella's
libc.a; `steps` = kernel instructions/faults/ns per call through perf_event_open; `whole <v>` for perf stat;
`fm <pages> <pos> <op>` = the same calls on a private file mapping; `spawn` = posix_spawn+waitpid loop). Scripts:
run1.sh/run2.sh/run3.sh (counts), time.sh (timing). Kernel 6.8.0-147 everywhere.

| step | kernel instr in the call, Broadwell-EP / Zen 1 | faults | N100 us in one start |
|---|---:|---:|---:|
| openat /proc/self/pagemap | 17.8k / 18.2k | 0 | 15.2 |
| pread 16 B | 3.7k / 3.8k | 0 | 2.7 |
| close | 1.9k / 2.0k | 0 | 1.1 |
| mremap page1 -> page0 | 33.1k / 33.5k | 0 | 15.4 |
| first call into the moved page | 0 | 0 | 0.09 |

Whole process (perf stat -r 200 x 3 rounds, medians, Broadwell-EP / Zen 1): guard alone +48k / +50k, mremap
alone +58k / +60k, both +96k / +97k; real start-up `no_classed_swap=0` vs `=1`: +65.5k / +66.6k.
Timing (spawn+wait of the static probe, 7 alternating rounds x 2000, medians): N100 swap off 187.4 us, on 216.7
(+29.3, +15.6%); guard alone +16.7, mremap alone +21.7, both +36.1 (vs no GLIBC_TUNABLES). Ivy Bridge (SSE2
class, no swap): 541.2 vs 541.6 us (no cost).

Candidates (measured, Zen 1 in-call): pair at the head 29.6k, middle 30.7k, tail 33.8k, lone 2-page VMA 35.7k:
the splits/+2 VMAs are not the cost. Pages not faulted in: -2.5k; populating gains nothing (no fault either way).
No guard mapping exists. Floor calibration (Broadwell-EP / Zen 1, in-call): mprotect 10.3k / 10.6k (cannot swap),
mmap MAP_FIXED 20.3k / 20.5k (needs an fd: +openat), munmap 29.3k / 30.0k, mremap 30.5k / 30.7k, getppid 0.4k.
Every call able to change page 0's mapping is >= 4x the 5k target. Lazy swap: needs a per-call check or a signal.

**Jobs (final code head 2bcd8655):** keke `classed-slot-swap-cost-lint` (clippy -D warnings + static) rc 0;
keke `classed-slot-swap-cost-plants` 3 CAUGHT / 0 SURVIVED / 0 WARN (handoff/classed-slot-swap-cost-plants.txt);
keke `classed-slot-swap-cost-gate` (affected --base origin/next) - see report json; counts keke
`classed-slot-swap-cost-split4`, kanon `classed-slot-swap-cost-bdw`; timing nozomi `classed-slot-swap-cost-n100`,
nico `classed-slot-swap-cost-ivb`. Superseded: keke `-split1` (probe bug, rc 139: read page 1 after the swap),
`-split2`, `-split3` (earlier probe versions; numbers consistent with split4).

**RULING NEEDED:** replace the pagemap guard with a user-space int3 (0xcc) scan of both pages (2.05k user
instructions, 0 kernel; neither page holds 0xcc today; drops ~48k kernel instr / ~16.7 us per start, and the
/proc dependency of #3140). Recommend: only if the swap is ever wanted on; on its own it leaves +44-60k, so keep
the default OFF and do not spend more on the swap until a slot shows a measured per-call gain (none so far).
**INFORMATIONAL:** any GLIBC_TUNABLES value costs Liella's static start-up +38k to +51k kernel instructions
(`glibc.malloc.check=0` +51k; `liella.compat.no_classed_swap=1` +38k; an unrelated env var 0) and +13.7 us on
the N100 (173.8 -> 187.4 us), +37 us on Ivy Bridge. Separate story worth cutting (profile first).

**Not to redo:** the per-call table, layout variants, populate, floor calibration (above).
**Exact next step:** coordinator rules on the guard scan; otherwise nothing: the swap stays off (D-4.53).
**Read:** D-4.53; docs/saturation/03 last row.
