# CLASSED-SLOT-INFRA handoff (one page)

**State:** branch `task/classed-slot-infra-3` (continues -2 and -1; real base = merge-base with next
`fbce527e`, 209 commits behind origin/next `e39b9799`). Code head `3b1821cc`. Role shiki:~/w/sh2 (Zen 1, AVX2 class).
**Verdict: DO NOT LAND.** The swap meets ruling #3139's system-call and VMA bound, but every start costs more
on every class (below).

**Decisions in force:** D-4.53 (classed entries, M6; section name `.text.liella_machine_memset` kept until the next
hot-text regen, ruling #3274), COMPAT-20 (`liella.compat.no_classed_swap=1`), rulings #3138-#3141, #3297.

**Run -3 (this run):**
* The -2 gate (mei `classed-slot-infra-2-gate`, FULL) failed in `affected::tests::csys_falls_back_to_full_when_unsure`:
  scan.S/fold.S escalated to FULL ("`pagemap` ... is a trait method"). Cause: `affected` follows a csys symbol's
  users by name, and the function `classed::page0()` (it reads `liella_classed_page0`) shared its name with the
  `page0` argument of `pagemap`, `install_with` and `decide`. Fix 3b1821cc: renamed it `page0_addr()`. Dev job
  `classed-slot-infra-3-dev` rc 0: xtask clippy, static, the 5 `affected::tests::csys*` tests (csys_symbol_reading
  included), classed unit tests (5), classed_slots (5). -2's count-branch never ran (it was chained after the red gate).
* Plants `classed-slot-infra-3-plants` rc 3: the tool REFUSED because its copy of the role lacked
  `conformance/build/s22_cancel_race.lock`. The gate I started at the same time in that role wrote that transient
  file. Not rerun (budget). Rerun AFTER the gate, alone in the role: same file handoff/classed-slot-infra-plants.txt.
* Start-up bound, shiki (AVX2 class), strace + VMA probe (`~/work/classed-slot-infra-3/vmas.c`): branch default
  = base + exactly openat(/proc/self/pagemap), pread64 16 B, close, mremap, and 13 VMAs vs 11. With `-AVX2` and with
  COMPAT-20: 28 calls / 11 VMAs, the same as base. **Within the bound.**
* **Over the bound: a fixed cost on every start, all classes.** Same host, release products, 2 passes x 5 runs:
  startup_kinstr_c merge-base 162.7-165.8k (default, COMPAT-20 and -AVX2 alike); branch 226.8-237.9k (default,
  COMPAT-20 and -AVX2 alike). startup_instr_c (user only) 2148 -> 2432 on every class. Inside the branch, swap on vs
  off is within noise, so the 4 calls are not the cost. Minor faults 60-63 -> 63-64 and text +11 KB (same LOAD
  segments) do not explain ~65k instructions. Cause NOT found. Job logs: shiki `classed-slot-infra-3-kin-{mb,branch}`.
* Counts vs origin/next (`classed-slot-infra-3-count-{base,branch}`, Liella instr/call): AVX2 class memset_144/256
  30.4 -> 28.4 (-2, as designed), others the same; SSE2 class _144/_256 39.4 -> 37.4; 16/64/128/4k the same.
* Booked timing (`timing` jobs, base staged by next's `bench --stage`, the branch by the manual recipe because the
  branch predates XTASK-BENCH-STAGE): Ivy Bridge (nico, SSE2 class) memset_16..4k time ratios identical to base
  in all 3 rounds (unchanged). N100 (nozomi, AVX2 class) memset at parity or a little better (_256 0.89 vs
  0.90-1.00), no slower row. N100 start-up: startup_kinstr_c time ratio vs glibc 0.52-0.56 base -> 0.60-0.65
  branch, slower, matching the instruction counts.

**Exact next step:**
1. Profile the fixed start-up cost before changing anything: per-phase split of one start (validation/wl_trace.sh)
   for merge-base vs branch, with `-AVX2` so no swap runs. Start with the +284 user instructions (they show what
   runs); then kernel instructions by phase (exec, faults, exit). Suspects to rule out: what `runtime.rs`/`config.rs`
   now run on every start, and text layout (the page-aligned classed pages in fold.S, hot-text order).
2. Fix it, rerun kin.sh on both trees (bound: branch = base on SSE2/COMPAT-20; AVX2 = base + 4 calls' cost).
3. Then gate (direct tier, `--base origin/next`), then plants alone in the role, then a re-timed N100 start-up row.

**Not to redo:** the export, guard and MDWE tests are in the conformance program; the syscall/VMA bound is
measured; memset timing on both classes is done (redo only if fold.S changes). No selftest symbol (libc.a localizes
hidden symbols). Scripts: shiki `~/work/classed-slot-infra-3/{counts,kin,faults,stage}.sh`; trees
`/mnt/zram/claude/classed-slot-infra-3/{base,mb,branch}`.

**Read:** docs/design/loadtime-entry.md 4 and 5.3-5.7; D-4.53; handoff/classed-slot-infra-plants.txt.
