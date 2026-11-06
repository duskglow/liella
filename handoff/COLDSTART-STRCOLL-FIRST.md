# COLDSTART-STRCOLL-FIRST: first strcoll in a rules locale

**State:** branch `task/coldstart-strcoll-first` (stacked on next e8189dd8), code + test + plants + D-4.54 committed and pushed.
The status is INCOMPLETE: gate, plants and counter bench are running detached on keke role `n2`. N100 timing has not run.

## Step 0 (re-time on next HEAD)
The planning story's nozomi job (CSP1106-n100, next 730f416b, `~/work/COLDSTART-PLAN-1106/counts-n100.txt`, DONE) gives these
probe deltas for "SC" - "S" (setlocale(LC_ALL, "en_US.UTF-8") then one strcoll(names[69], names[70])):

| side | user instr | kernel instr | faults |
|---|---|---|---|
| Liella (next) | +11,781 | +31,875 | +3 |
| glibc | +348 | +1,805 | 0 |

The partial bench there (bench.log) has Liella first_strcoll = 11,367 ns/op on the N100, but it has no glibc ratio for the
first_* rows. The gap is far above 1.05x, so the code change went ahead. The perf-record split by symbol (brief "confirm the
split first") was NOT done, for budget reasons. The hypothesis (the full-slot fill) is not confirmed by profile.

## Change (D-4.54)
`crates/core/src/locale/collate.rs`: `first_level` calls `warm_for(a.table_bytes(), b.table_bytes())`. The first
general-path compare of a catalog fills the lane/unit entries of its own ASCII bytes plus byte 0 only (`AsciiSlot::known`,
2 x u64, set with Release after the entries). A later general-path compare that finds the slot not ready runs the old full
`fill`. `finish` (ascii_fast, rank, ready) runs once all 128 bytes are known. Wide texts (`table_bytes` = None) fill nothing.
Hot paths are unchanged: still one Acquire load of `ready`. `ascii_lane_bytes` still does a full fill (test API).
New test API: `ascii_known_bytes() -> (known, ready)`.

Behaviour change to note: after the first compare, `decide_first` decides nothing until a second general-path compare
completes the tables; `compare_stream` answers from the known bytes meanwhile. The second call on the same pair
(cbench second_strcoll) therefore goes through compare_stream instead of the rank path. Its row must be checked in the
timing run.

## Tests and plants
- `crates/core/tests/collate_lazy.rs`: the existing test now expects (128, true) only after the second general-path compare.
  New test `first_comparison_fills_only_its_bytes_and_orders_as_a_full_fill` (spec App. D.1.4, D-4.18, D-4.54) checks three
  things: the first compare orders as on a fully filled copy and as the keys do; only the pair's distinct bytes plus the
  terminator are known; and compare_stream over strings of those bytes equals the full copy's answers.
- On keke n2: `cargo xtask clippy` rc=0, `cargo xtask static` rc=0, `cargo test -p liella --test collate_lazy` 2 passed, no skip.
- Plants: `handoff/COLDSTART-STRCOLL-FIRST-plants.txt` (lane-wrong-on-demand, ready-before-all-known, skip-rank-build).

## Jobs (keke, chain `~/work/COLDSTART-STRCOLL-FIRST/chain.sh`, log chain.log, DONE file at the end)
- `keke:csf-gate`: `~/bin/gate 12G cargo xtask affected --base origin/next` selected 104 targets but RAN NONE (no `--run`), rc=0: not a gate.
- `keke:csf-gate2`: the same with `--run` (the real direct-tier gate). Log: ~/logs/csf-gate2.log
- `keke:csf-plants`: `cargo xtask plants handoff/COLDSTART-STRCOLL-FIRST-plants.txt --dir /dev/shm/claude/COLDSTART-STRCOLL-FIRST/plants-1`
- `keke:csf-counts`: `cargo xtask bench --compare --counters cbench_opt` (instructions only: keke is a build host, not a timing host)

## NOT VERIFIED / next step
1. Collect the three jobs. If all are green and the plants are CAUGHT, drop the "WIP:" status (re-commit the subject is not needed;
   the coordinator decides).
2. Probe counts after the change. The static `coldsteps.liella` (copied to keke `~/work/COLDSTART-STRCOLL-FIRST/bin/`) is
   linked against next's libc.a, and its source was not found. Relink it against this branch's libc.a, or write an S/SC probe,
   then compare the SC - S deltas against the table above.
3. N100 timing: book nozomi, stage with `cargo xtask bench --stage` on a build host, then run `--staged --only cbench_opt`.
   Rows: first_strcoll, second_strcoll, strcoll_en_us_*.

Do not redo: step 0 (the numbers above are on next 730f416b).
Plan rows: D-4.30, D-4.37, D-4.54 (plan/07-decisions/s04-byte-arrays-and-strings.md).
