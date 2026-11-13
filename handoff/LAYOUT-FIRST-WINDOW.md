# LAYOUT-FIRST-WINDOW: handoff (after LAYOUT-FIRST-WINDOW-2)

State: branch `task/layout-first-window-2` (origin/next eed0052ec merged in), head in
handoff/reports/LAYOUT-FIRST-WINDOW-2.json. Build role mei:~/w/s1 (Skylake-SP). Scratch on mei:
`~/work/LAYOUT-FIRST-WINDOW-2/` (fw2-probe.sh, cmp.sh, htp-<base|head>-<profile>.log).

## Decisions in force

- D-X.79 (records/decisions/D-X/D-X.79.toml): extends D-X.63 (clusters, `@place end`) and D-X.77; D-X.73/74 for
  the data side of the start-up layout.
- `crates/libc-so/hot-text.first-window` (hand-kept input): keys `cargo xtask so` links right after the start-up
  tier. Holds `liella_sys_fstat`. Stale lines fail `cargo xtask static` and `so`; a `--hottext-regen` build reports them.
- Since the merge with next (HOTLIST-REGEN-CHEAP): `hottext::group` takes the hand-written `inputs` directory
  (crates/libc-so); the first-window list and the clusters are read from there, never next to the list (the list
  may be a train-owned copy under target/hot-lists, where the first-window file does not exist and would have been
  silently skipped). `static_check` reads both from crates/libc-so too.
- Contract: `init_startup_group_ends_inside_the_first_fault_around_window` (hot_text_products) fails with
  `FIRST-WINDOW margin` or `FIRST-WINDOW liella_sys_fstat`.

## Measured on the merged head (hot_text_products, min of 5 runs, every phase; base = next eed0052ec in RAM)

| probe | release next -> head | fast next -> head |
|---|---|---|
| tool | 56 -> 55 | 54 = 54 |
| locale | 53 -> 52 | 51 = 51 |
| localtime | 19 -> 18 | 19 -> 18 |
| localtime_mktime | 19 -> 18 | 19 -> 18 |
| true, fork, float, mktime, strings, rebind | equal | equal |

Text/data phases: release 10 of 68 better, 0 worse; fast 0 worse. Jobs mei:lfw2-probe-base, lfw2-probe-head (both
rc 1 only on the release short-program bound below, red on next too; evidence). Proof jobs: lfw2-lint1 (clippy +
static), lfw2-plants (2 CAUGHT, handoff/LAYOUT-FIRST-WINDOW-2-plants.txt), lfw2-gate (escalated to FULL),
lfw2-fwcheck (FIRST-WINDOW test, both profiles).
Margins: release 1844 bytes (fstat 0x4178c0..0x4178cc), fast **180 bytes** (0x417f40..0x417f4c), window
0x408000..0x418000.

## Open / INFORMATIONAL

- Pre-#5026 values (tool 52, locale 50; the last run here measured head 53/50/17/17 on next a5636ce6b) are NOT
  reached: next itself gained +2 release faults on tool and locale, +1 on localtime and localtime_mktime since a5636ce6b,
  independent of this branch, which still removes one each. On next, the release profile fails
  `init_short_programs_stay_within_their_page_fault_bounds` (setlocale + printf 53 > 51); the head gets 52 > 51, still
  red. Fast passes. The next growth is not chased here (START-UP LAYOUT JITTER rule).
- Fast margin fell from 1012 to 180 bytes with next's start-up growth: the next 180 bytes of start-up text in fast
  fail FIRST-WINDOW. Then shrink the start-up tier (printf `to_string` body ~9.8 KiB) or move rarely-first keys out of
  the head (`puts`/`puts_general`, untraced `extra -` keys); never widen the window.
- The test reads crates/libc-so/hot-text.list like the other products tests; under a train-owned list copy in
  target/hot-lists the start-up count it uses could differ from the build's.

## Exact next step

Land after lfw2-gate, lfw2-plants and the probes. Follow-up story: find next's release start-up growth since
a5636ce6b (+2 tool/locale) and restore the release short-program bound; widen the fast first-window margin.
