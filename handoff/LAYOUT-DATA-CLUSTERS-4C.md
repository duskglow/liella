# LAYOUT-DATA-CLUSTERS-4C: -4B's layout under the start-up instruction bound

**State (2026-11-11):** branch `task/layout-data-clusters-4c` (on 4B's head `6ca498fec`, which already contains
origin/next `28a903a01`; no new merge was needed). Code commits: `e23e52698` (scan change + plants file) and
`2e565c251` (clippy fix: `wrapping_mul`). Role keke:~/w/n1 (Zen 1, EPYC 7551, the instr_limits CPU).
Decisions in force: D-X.64, D-X.67, D-X.68 (from -2..-4). None added: the change is local and testable (a
one-loop rewrite with the same results), and the brief forbids editing plan/07-decisions.

## Attribution (keke, `cargo xtask bench --counters --only cbench_startup`, 3 runs each, all equal)

| tree | startup_instr_c | printf | setloc | wctype |
|---|---|---|---|---|
| origin/next 28a903a01 (ldc4c-startup-next) | 2146 | 71874-71875 | 68079-68085 | 69099-69108 |
| 4B head 6ca498fec (ldc4c-startup-4b, stopped at run 1 by the limit) | **2156** | - | - | - |
| this head 2e565c251 (ldc4c-startup-head-2) | **2142** | 71849-71861 | 68056-68063 | 69080 |

Next alone is under 2150, so the +10 comes from 4B. Found with a gdb single-step split of one `cbench_startup true`
child per function (scratch `~/work/LAYOUT-DATA-CLUSTERS-4C/trace_start.py`, ASLR off: next 2169, 4B 2182 under
gdb). All +13 was in `runtime::start::<false>`, and readelf shows why: 4B's static executable has **8 program
headers instead of 7**. The 64 KiB `.rodata1` alignment leaves a gap of more than a page after `.rodata`, so ld
starts a second read-only PT_LOAD. `liella_rt::start::program` (inlined into the start) cost exactly 13
instructions per header: two loop bounds (`chunks_exact` plus `take(phnum)`), then three type compares.

## Change
`crates/rt/src/start.rs::program`: cut the table to `min(phnum, len/7)` whole headers once (so `chunks_exact`
is the only bound), and leave a PT_LOAD after one compare. The results are unchanged (first header of each type,
`phnum` and a truncated table still limit the read; the existing `start::` tests cover all three). The layout and
its fault gains are untouched: no section, alignment or key changed.

## Not measured here (NOT VERIFIED)
Start-up page faults were not re-measured on this head. The change touches only the start's code (the loop is a
few bytes shorter) and no data or layout, so 4B's table (fast -12, release -13 vs next, mei) should still hold.
The coordinator or -5 can confirm it with 4B's `~/work/LAYOUT-DATA-CLUSTERS-4B/counts.sh` on mei.

## Jobs (keke)
- ldc4c-startup-next (rc 0), ldc4c-startup-head-2 (rc 0), ldc4c-lint-2 (clippy + static, rc 0).
- ldc4c-plants-3 (3 plants, `handoff/LAYOUT-DATA-CLUSTERS-4C-plants.txt`, run from a copy in ~/work, same code) and ldc4c-gate-2 (`affected --run
  --base origin/next`): queued in order by `~/work/LAYOUT-DATA-CLUSTERS-4C/chain.sh`, pending at hand-back.
- Superseded: ldc4c-startup-4b (diagnostic: rc 1 because 4B is over the limit, the point of the measurement),
  ldc4c-trace-next / ldc4c-trace-4b (diagnostic traces), ldc4c-lint (clippy refused the plain multiply; fixed in
  2e565c251), ldc4c-plants-2 (rc 3: the tool refused bare s/// patterns; perl: prefix added in b16ff1fbd), ldc4c-startup-head (stopped with the chain after that lint failure).

## Next step
-5..-7 as planned in `handoff/LAYOUT-DATA-CLUSTERS-4B.md` (release `strings` fault, fast `float` +1, -4 item 1).
A later layout that adds a PT_LOAD now costs about 7 instructions per header in this loop, not 13. The CI limit
of 2150 now has 8 instructions of headroom. Tightening it to the new level is the coordinator's call (the bound is
shared with the train).
