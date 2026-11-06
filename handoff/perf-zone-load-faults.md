# PERF-ZONE-LOAD-FAULTS(-2): the first localtime_r's faults (zone load)

State: branch task/perf-zone-load-faults-2 (continues task/perf-zone-load-faults, stacked on
PERF-LOCALTIME-FIRST-SECOND-MKTIME). Code commits: da632ecd (one allocation per table, run 1), 61ac20c4 (hot-text
pin, run 2). Role run 2: mei:~/w/s1 (Skylake-SP). Decisions in force: D-23.6, D-7.14. No new decision row (none
reserved; the pin is a local layout detail recorded here and in the test comments).

## Counts (fault oracle crates/core/tests/oracle/page_faults.c, static build, no ASLR, every run identical)
| mode | before run 1 | after run 1 | after run 2 (61ac20c4) |
|---|---|---|---|
| localtime (first localtime_r, load included) | 25 (Zen 1) | 22 (Zen 1 and Skylake-SP) | **20** (Skylake-SP) |
| mktime (first mktime after it) | 0 | 1 (gate red) | **0** |
| localtime_mktime (new mode: both together) | - | (22 + 1 per call) | **20** |
glibc's first localtime_r: 4 (original brief). Probe: mei ~/work/PERF-ZONE-LOAD-FAULTS-2/probe.sh (jobs
perf-zone-load-faults-2-probe-a = inherited head, -probe-b = pinned).

## Run 2: what changed
* Cause of run 1's red gate (perf page-fault addresses): `civil::days_from_civil64` sat in the hot group behind
  strftime's functions, in the next 64 KiB window after the localtime block (0x450000 vs 0x44xxxx); the old longer load
  happened to touch that window, the leaner one does not.
* `crates/libc-so/hot-text.list`: the mktime path (mktime, zone::mktime, implicit, resolve_in, days_from_civil64)
  moved from behind strftime to right behind the localtime block; four zone-load functions the list lacked (new or
  renamed since the last regen) added to the block: `time::zone_path`, `TryVec<(i64,u8)>::resize` (transitions),
  `rule::when_of`, `Zone::kind_period`. Not moved: `tsc_probe`/`liella_sys_prctl`/`rdtsc` and legacy `finish_grow`
  (shared with gettext and nss paths; moving them could cost those programs a window: untested idea, next step).
* `tools/hottext/probes/p_cold.c`: mktime before strftime, so a regen produces the same order (merge.py: the new
  trace's first-touch order wins).
* Tests (hot_text_products.rs): new `init_first_localtime_r_and_mktime_stay_within_their_combined_page_fault_bound`
  (LOCALTIME_MKTIME_BOUND 22 = 20 + 2) beside the unchanged per-call tests; LOCALTIME_BOUND 24 -> 22. The mktime test
  (bound 0) not loosened.
* Plants: handoff/perf-zone-load-faults-2.plants (mktime-pin-lost: the mktime test must fail; rule-years-grown: table
  x4, the combined test must fail).

## RULING NEEDED: rule-years zeroed allocation (coordinator ruling #3727's condition is NOT met)
There is no existing TryVec -> boundary allocation hook: `TryVec` wraps `alloc::Vec` and its `try_reserve*` reach the
global allocator directly; core (`forbid(unsafe)`, generic over `Sys`) cannot call liella-boundary (boundary depends
on core). A fallible zeroed table therefore needs (a) a NEW seam (a `Sys` method or an injected function) and (b) a new
kind of unsafe outside the `allocator` category (`unsafe impl GlobalAlloc` only):
```rust
// liella-boundary heap.rs (proposed, not built)
pub fn try_zeroed_slots<T: ZeroValid>(n: usize) -> Option<Vec<T>> {
    let layout = core::alloc::Layout::array::<T>(n).ok()?;
    if layout.size() == 0 { return Some(Vec::new()); }
    // UNSAFE(allocator): stable Rust has no fallible zeroed Vec; the block has `layout` from the global allocator,
    // all bytes zero, and `ZeroValid` (sealed: AtomicI64, i64, u8 ...) means all-zero is a valid T, so the Vec owns
    // n initialised elements with the allocation it will free.
    #[allow(unsafe_code)]
    unsafe {
        let p = alloc::alloc::alloc_zeroed(layout).cast::<T>();
        if p.is_null() { return None; }
        Some(Vec::from_raw_parts(p, n, n))
    }
}
```
plus `YearSlot { start: 0 sentinel }` in zone.rs. Gain: 2-3 heap faults on the first localtime_r of a zone with a DST
rule (the 4.8 KiB table stays untouched until a year past the last transition is asked). Recommend: the owner rules
on the seam; until then nothing changes.

## Not done (run 2)
* Step 5 of the original brief (booked timing of the time-group rows on Ivy Bridge and the N100, beside the
  PERF-FIRSTCALL-MKTIME-SNPRINTF-2 numbers): not started; a booking waits 30-60 min and the budget went to the fix.
* Zen 1 host count of the pinned layout (only Skylake-SP measured in run 2; the bounds keep 2 of margin).

## Remaining 20 faults of the first localtime_r (from run 1's split, minus the pinned text)
Text: the localtime block's window, tsc_probe (0x47 window), legacy finish_grow (0x48). vDSO data 1, stack 1. Heap ~13:
allocator metadata (new size classes), path, file bytes, transitions, types/names, rule-years 2 pages, copy(tz), Entry box.

## Jobs (run 2, mei)
lint0 (clippy + static on the inherited head: green), probe-a / probe-b (counts above), gate2 + plants2 on the final
head. SUPERSEDED: plants (first plants file: mktime-pin-lost SURVIVED, since unpinning days_from_civil64 alone leaves it
in ordinary text in a window something else already touched (not verified which); replaced by
mktime-block-behind-strftime, which restores run 1's order), gate (stopped by me: escalated to FULL because
hot-text.list is a product linker input, restarted on the final head as gate2).

## Next step
1. Booked timing (step 5) of first_localtime_r / first_mktime on kotori or nico (less busy) and the N100.
2. Try moving tsc_probe+prctl+rdtsc into the localtime block and measure every hot_text_products mode (gettext too).
3. copy(tz) + Entry box + path inline in the Entry (three small heap blocks); file bytes + transitions as one allocation.
4. Rule-years zeroed table after the ruling above.
