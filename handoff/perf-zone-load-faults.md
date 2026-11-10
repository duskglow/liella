# PERF-ZONE-LOAD-FAULTS(-2, -3): the first localtime_r's faults (zone load)

State: branch task/perf-zone-load-faults-3 (continues -2 and -1, stacked on PERF-LOCALTIME-FIRST-SECOND-MKTIME),
origin/next 66b8d3ba merged in (73928b7c). Code commits: da632ecd (one allocation per table, run 1), 61ac20c4
(hot-text pin, run 2), 73928b7c (merge, list conflict resolved, run 3), 5225ce67 (bounds 22 -> 20, run 3).
Role run 3: keke:~/w/n1 (Zen 1, EPYC 7551). Decisions in force: D-23.6, D-7.14; owner ruling #3732 (no
zeroed-allocation seam; rule-years table unchanged; future work ZEROED-TRYVEC-SEAM). No new D-row (none reserved;
the list layout is a local detail recorded here and in the test comments).

## Counts (fault oracle crates/core/tests/oracle/page_faults.c, static build, no ASLR, every run identical)
| mode | before run 1 | after run 1 | after run 2 | after run 3 (merged, Zen 1) |
|---|---|---|---|---|
| localtime (first localtime_r, load included) | 25 (Zen 1) | 22 (Zen 1, Skylake-SP) | 20 (Skylake-SP) | **18** |
| mktime (first mktime after it) | 0 | 1 | 0 | **0** |
| localtime_mktime (both) | - | 22 + 1 | 20 | **18** |
glibc's first localtime_r: 4 (original brief). Run 3 did not measure the pre-merge head on Zen 1 (run 1 read the
same count on both classes) nor the merged head on Skylake-SP. Probe: keke ~/work/PERF-ZONE-LOAD-FAULTS-3/probe.sh
(job -probe-m); mutant counts mutants.sh / mutants2.sh (jobs -mutants, -mutants2, in the run's RAM plant copy).

## Run 3: what changed
* CI CONFLICT on crates/libc-so/hot-text.list: next (PERF-FIRSTCALL-MKTIME-SNPRINTF-2 and others) added
  tsc_probe, liella_sys_prctl, liella_machine_rdtsc, Resolution::matches, TscGate::fresh inside the mktime path
  behind strftime; this branch had moved that path behind the localtime block. Resolved by hand (regen.sh re-traces
  every probe with gdb and reorders far more): the mktime block stays behind the localtime block and carries next's
  five entries in next's order (after implicit, before resolve_in). `TryVec<(i64,u8)>::resize` was listed twice
  after the merge; next's position (after tzif::block) kept. This realised run 2's next step 2: tsc_probe now sits
  in the localtime window (0x44 on Zen 1), so the first localtime_r reads 18.
* First plants run (on 73928b7c): both plants SURVIVED: bounds 22 had 4 faults of slack. Measured mutants (Zen 1):
  mktime block behind strftime = 19 / 0 / 19 (tsc_probe leaves the window; mktime still finds its text, so the
  per-call mktime test no longer sees it); rule-years x4 = 21 / 0 / 21; block dropped from the list = 21 / 2 / 23.
* hot_text_products.rs: LOCALTIME_BOUND and LOCALTIME_MKTIME_BOUND 22 -> 20 (18 + 2, the file's rule).
* Plants handoff/perf-zone-load-faults-3.plants: mktime-block-dropped, rule-years-grown. Run 2's
  behind-strftime plant is not kept: a one-fault layout move is inside the bounds' 2-fault margin by design
  (INFORMATIONAL in the report; a margin of 1 would catch it, but the merged head is unmeasured on Skylake-SP).

## Not done
* Booked timing of first_localtime_r / first_mktime (Ivy Bridge, N100): deferred to the next sweep (coordinator
  note; the merge changed only hot-text order and test bounds, no code on the time path).
* Skylake-SP count of the merged head (CI runs there; bounds keep 2 of margin over the Zen 1 count).

## Remaining 18 faults of the first localtime_r
Text: the localtime block's window, legacy finish_grow (0x48 window in run 1). vDSO data 1, stack 1. Heap ~13:
allocator metadata (new size classes), path, file bytes, transitions, types/names, rule-years 2 pages, copy(tz),
Entry box.

## Jobs (run 3, keke)
lint0 (xtask clippy + static on the inherited head: green), probe-m, mutants, mutants2 (evidence), then on the final
head: lint1, gate2 (escalates to FULL: hot-text.list is a product linker input), plants2. SUPERSEDED: gate (FULL, on
73928b7c, stopped by me when the bounds changed), plants (on 73928b7c: 2 SURVIVED, led to the bound change).

## Next step
1. Skylake-SP count of the merged head; if it reads 18 too, consider margin 1 so a one-fault layout move is caught.
2. Booked timing in the next sweep.
3. copy(tz) + Entry box + path inline in the Entry (three small heap blocks); file bytes + transitions as one
   allocation; legacy finish_grow's window.
4. Rule-years zeroed table: only if the owner revisits ZEROED-TRYVEC-SEAM (#3732 declined it).
