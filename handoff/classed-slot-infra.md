# CLASSED-SLOT-INFRA handoff (one page)

**State:** branch `task/classed-slot-infra-4` (continues -3/-2/-1; origin/next `41b8737b` merged in at `c97fa7bc`, so
the story's base is now origin/next; profile comparisons use the old merge-base `fbce527e` tree). Code head `d6838ffc`,
plants `7a3c12b9` tree. Role shiki:~/w/sh2 (Zen 1, AVX2 class).
**Verdict: INCOMPLETE.** The fixed start-up cost is found and removed. Still to do: the N100 start-up timing, and
settling a possible ~+8k kernel-instruction difference on the no-swap classes (below).

**Decisions in force:** D-4.53 (classed entries, M6; row updated: the class comes from `cpu::init_with`'s state),
COMPAT-20 (`liella.compat.no_classed_swap=1`), rulings #3138-#3141, #3297.

**Run -4 (this run):**
* **-3's "every class pays +65k" was a measurement error.** cbench_startup gives its children `PATH` only, so
  `bench --tunables` never reached the measured start: -3's COMPAT-20 and -AVX2 rows were the default class with
  the swap. With the tunables in the child (`~/work/classed-slot-infra-4/prof3.sh`), COMPAT-20 and -AVX2 make
  the same system calls (9 / 12) and faults as the merge-base. Fixed: the bench now passes `GLIBC_TUNABLES` to its
  children (d6838ffc).
* **The real fixed cost (+80 user instructions, every class)** came from an exact single-step tracer
  (`~/work/classed-slot-infra-4/step.c`, one RIP per user instruction, mapped to symbols with nm). `classed::start`
  called `cpu::features_cached()` a second time. With a second caller, `features()` went out of line
  (+169 in `cpu::features`, -89 in `init_with`). Fix d6838ffc: `init_with` already returns its state byte.
  `runtime::start` passes `cpu::avx2_in(state)`, and the class rule is `cpu::avx2_wanted`, used by `init_with`
  (`classed::avx2_class` is removed).
* **After the fix** (job `classed-slot-infra-4-r2-prof`, exact user instructions, merge-base -> branch):
  SSE2 class (-AVX2) 5330 -> 5338 (+8: runtime::start +10, resolve_fold_slow +5, strlen -7); COMPAT-20
  5622 -> 5826 (+204: `key_of` +120 and `signed_number` +54 parse the flag that the merge-base ignores, start +41);
  default AVX2 class 2145 -> 2319 (+174, the swap's code) and 9 -> 13 system calls. Kernel, perf stat, 5 runs each:
  default 197k -> 270k (the swap's 4 calls, within #3139); COMPAT-20 218k -> 227k and -AVX2 247k -> 255k in this
  run, but equal in the earlier run (`prof3.sh`: 215-220k vs 213-220k, 241-247k vs 237-254k). Run-to-run spread
  is about ±8k at load ~15, so that gap is NOT settled. Bench rows on the role (now with tunables): startup_kinstr_c
  224.8k default / 192.4k COMPAT-20 / 209.3k -AVX2. startup_instr_c 2331 / 5870 / 5356.
* Gate/plants: `classed-slot-infra-4-r2-dev` rc 0 (full clippy, xtask clippy, static, classed unit tests, bench build);
  `-r2-gate` (direct tier) then `-r2-plants` (alone in the role, `/dev/shm/claude/CLASSED-SLOT-INFRA-4/plants-1`), chained
  by `~/work/classed-slot-infra-4/chain.sh`; results in the report json.

**Open:** (1) is the ~+8k kernel on no-swap classes real? Use many interleaved runs (perf stat -r 50, mb and branch
alternating) on a quiet moment; suspects are the +11 KB text (page-cache/readahead of the bigger LOAD) and the
page-aligned classed section. (2) COMPAT-20 parsing costs 174 user instructions in `key_of`/`signed_number` whenever
the flag is set. It is the cost of honouring the flag, paid only by those who set it; `key_of`'s 120 could shrink.

**Exact next step:** 1) check -r2-gate and -r2-plants (fix any red); 2) settle the kernel gap (Open 1); 3) re-time the
N100 start-up row (stage per handoff/timing-sweep-1105.md, now `cargo xtask bench --stage` works on this branch; run
`~/bin/timing <label> ...` directly on the N100 host, base = origin/next).

**Not to redo:** the syscall/VMA bound is measured; memset timing on both classes is done (redo only if fold.S changes);
the fixed user cost is found and fixed. Scripts: shiki `~/work/classed-slot-infra-4/{step.c,prof3.sh,prof5.sh,prof6.sh,chain.sh}`;
merge-base tree `/mnt/zram/claude/classed-slot-infra-3/mb`.

**Read:** docs/design/loadtime-entry.md 4 and 5.3-5.7; D-4.53; handoff/classed-slot-infra-plants.txt.
