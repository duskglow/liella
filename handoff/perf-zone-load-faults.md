# PERF-ZONE-LOAD-FAULTS: the first localtime_r's heap faults (zone load)

State: branch task/perf-zone-load-faults (stacked on PERF-LOCALTIME-FIRST-SECOND-MKTIME, head 498fa5f9 inherited),
code commit da632ecd. Role keke:~/w/n1 (Zen 1). Decisions in force: D-23.6, D-7.14. No new decision row (none reserved;
the change is local: sizing of private tables).

## What was measured (step 1)
Oracle `page_faults localtime` (new mode in crates/core/tests/oracle/page_faults.c: TZ=America/New_York, ru_minflt
around the first localtime_r only), static Liella build, `setarch -R`, 6 runs; perf record of page-faults with
addresses on the Liella binary only (scratch probe keke ~/work/perf-zone-load-faults/probe.sh, outputs faults-base.txt,
faults-new.txt). Every run identical: **25 before, 22 after.** (This oracle starts with a nearly empty heap; the
predecessor's 18 was the bench's cold child, where earlier calls already opened some heap regions.)

Before: 13 heap faults = 4 allocator metadata pages (one per refill of a new size class) + 9 data pages, one per size class:
the path (grown 19 -> 38 bytes: realloc copy), the file bytes (3553), the transitions (3776), the parser's temporary
TypeRecord table (144), the zone's types, the names table (grown 8 -> 16 -> 32: realloc copies), the rule-years table
(4.8 KiB filled with the sentinel at load: 2 pages), the legacy `copy(tz)` and the Entry box.

## What changed (step 2)
* tzif.rs: the type records stay in the file's bytes (`Tzif::type_records`, read with `types()`), checked in place;
  no temporary table.
* zone.rs `from_tzif`: the names table is reserved once (designations + footer names bound), the types exactly.
* time.rs `zone_path`: the exact length reserved up front.
* The header counts were ALREADY bounded by the file length before any table is sized (`block` checks `block_len`
  first; the transitions were already sized from the header). Not changed; now pinned by a test.

## Remaining 22 faults (step 3), each with its cause
Text pages (7): localtime_r, zone_path, TryVec::resize, rule::when_of (footer parse), tsc_probe, legacy finish_grow,
kind_period. vDSO data (1: first clock read). Stack (1: the load's frame). Heap (13 -> 10 -> listed): metadata 4 pages
(new classes), path block, file bytes, transitions, zone types/names (2 regions), rule-years table 2 pages, copy(tz),
Entry box.
Not fixed here, with why:
* rule-years (2-3 faults): a zero sentinel plus a fallible ZEROED allocation would leave the fresh pages untouched until
  a year past the last transition is asked. The core crate is forbid(unsafe) and stable Rust has no safe fallible zeroed
  Vec, so it needs a hook from liella-boundary (e.g. `TryVec::zeroed`): **RULING NEEDED** (touches the unsafe boundary;
  recommend the hook, it also serves other lazily filled tables).
* file bytes + transitions as one allocation: the zone would keep the file bytes and decode big-endian transition times
  on lookup; touches every lookup in zone.rs: a story of its own (also saves the parse loop's user instructions).
* copy(tz) + Entry box + path: three small blocks in three classes; could live inline in the Entry. Next step.
* text faults: a hot-text pin of the zone-load functions (not a full regen: NEGATIVE row in docs/saturation/07).

## Tests (step 4)
* hot_text_products `init_first_localtime_r_stays_within_its_page_fault_bound`: LOCALTIME_BOUND 24 (22 + 2 margin).
* zone_load_alloc.rs (ALLOC-FAIL-INJECT hook, no unsafe): `a_zone_load_makes_one_allocation_per_table` (from_tzif <= 3
  allocations, time::load <= 5) and `tzif_counts_larger_than_the_file_are_rejected_before_any_table_is_allocated`
  (six lying headers: EINVAL, no allocation of a page or more).
* Plants: handoff/perf-zone-load-faults.plants (names-grown, path-grown, no-header-bound, type-offset-endian).

## Not done
* Step 5 timing (booked Ivy Bridge + N100 runs of the time-group rows): not started, budget. Next run books it.
* Bench cold-child fault count (the predecessor's 18) not re-measured; the oracle shows -3.

## Jobs (keke)
perf-zone-load-faults-probe-base (before), perf-zone-load-faults-probe-new (after), perf-zone-load-faults-gate
(clippy, static, affected --base origin/next), perf-zone-load-faults-plants.

## Next step
Book kotori/nico and nozomi for the time-group rows; then the rule-years zeroed-allocation hook after the ruling.
