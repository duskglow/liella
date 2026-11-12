# LAYOUT-HOTKEY-INLINE-STABLE (-4): handoff

State: branch `task/layout-hotkey-inline-stable-4` (stacks on -3; base next d1b3a596c, not merged), head in
handoff/reports/LAYOUT-HOTKEY-INLINE-STABLE-4.json. Build role keke:~/w/n1 (Zen 1). Scratch on keke:
`~/work/LAYOUT-HOTKEY-INLINE-STABLE-4/` (lhis4.c percall spec, probe-job.sh, fmap.sh, faults.txt, fmap-*.txt).
-1..-3 history: the -3 version of this file (on `task/layout-hotkey-inline-stable-3`).

## Decisions in force

- Rulings #4997, #5024, #5025, #5068 (the 241 plain functions enter hot-text.unpinned as the ratchet's start with
  "boundary cost not measured yet"), #5069 (the coordinator assigns the D-X id: record
  `records/pending/LAYOUT-HOTKEY-INLINE-STABLE-3-1.toml`, points (4) and (5) added in -4).
- `cargo xtask static` (`check_pinned`): every Rust key of every tier is `#[inline(never)]` or has a
  `<key>\t<tier>\t<reason>` line in `crates/libc-so/hot-text.unpinned` (unchanged in -4).
- NEW (-4): a key whose fixed boundary costs instructions is not pinned; its function is `#[inline(always)]` into its
  callers and `cargo xtask hottext-repair --at <rev>` drops the dead key (no key left for a growth to kill).
- Coordinator 11-10: #5026 (release start-up text fault, `liella_sys_fstat` 0x240 B past the first fault-around window)
  lands as is; LAYOUT-FIRST-WINDOW (priority 96, held until this lands) fixes it structurally.

## Done in -4 (keke logs `~/logs/<label>.log`)

- `lhis4-lint0` rc 0: clippy + static on the inherited head.
- Counts (wl_percall, instructions:u per call, base = next merge-base d1b3a596c, spec lhis4.c):
  - `nscache::real_secs` pin: free (getpwnam_r_root 5688.6 = next 5688.6; getservbyname_r 12413.5 vs 12412.7). Kept.
  - `iconv::load` pin: +35/+34 (iconv_short 779 -> 814, iconv_reset 251 -> 285). Now `#[inline(always)]`, key dropped
    (repair): iconv_short 779 = next, iconv_reset 251 = next (`lhis4-percall-exp`, `lhis4-percall-bisect`).
  - `locale::builtin` pin: about +25 on setlocale_ctype_posix (pinned 2438.7 / 2447.2, unpinned 2419.7 / 2417.2;
    sd 7-8); setlocale_all_c and setlocale_query equal next. Now `#[inline(always)]`, key dropped (repair).
- Products + hot_text_products per phase, both profiles, on head 785f7a604 (`lhis4-probe-head`), on next d1b3a596c
  (`lhis4-probe-base`) and on head + dummy libm growth (`lhis4-probe-headdummy`): all `so` ok, all hot_text_products
  ok. No dead key in either profile with the dummy growth. Fast: every probe equal to next. Release: tool 54 vs 53,
  locale 51 vs 50 (bound 51), localtime 18 vs 17, localtime_mktime 18 vs 17; every other probe equal. Fault map
  (`lhis4-fmap3`, fmap.sh): in tool, locale and localtime the ONLY extra fault is the `.text` page 0x418000 (first
  page past the first fault-around window, 0x408000..0x417fff); every other section count equals next. One cause:
  #5026, which shows in every release probe that calls fstat, not only tool. The dummy run has the same numbers.
- Gate and plants: see the report json (`lhis4-gate`, `lhis4-plants`, file handoff/LAYOUT-HOTKEY-INLINE-STABLE-4-plants.txt).

## Open / RULING NEEDED

- #5026 also adds +1 to the release locale (50 -> 51, at its bound 51), localtime and localtime_mktime probes (same
  page). The 11-10 decision named only the tool probe. Recommend: accept them under the same decision (one fault, one
  fix in LAYOUT-FIRST-WINDOW), and tell LAYOUT-FIRST-WINDOW to check all four probes.
- setlocale(LC_CTYPE, "POSIX") per call is +170 over next on the -2 head already (next 2244.2 sd 0.9; -2 head 2417.2;
  final head ~2420-2447, sd 7-8), before any -3 pin; not caused by the -3/-4 changes. Only -1/-2 change pins and lists
  (start-up keys, data keys); no change on the setlocale path, and the spread grew from 0.9 to 7-8, so the extra count
  depends on addresses (a layout effect, likely stack or data alignment in the string kernels). Not bisected further
  (budget). Recommend: a short follow-up story to attribute it (wl_percall with --base 38e278e6f, the -1 head; then
  wl_trace of one call), before or with LAYOUT-FIRST-WINDOW.

## Exact next step

Coordinator: rule on the two items above; then land (the gate and plants named in the report). Follow-ups: pin the
"boundary cost not measured yet" keys in batches with counts (#5068); LAYOUT-FIRST-WINDOW.
