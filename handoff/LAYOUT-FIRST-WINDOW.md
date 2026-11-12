# LAYOUT-FIRST-WINDOW: handoff

State: branch `task/layout-first-window` (base next a5636ce6b), head in handoff/reports/LAYOUT-FIRST-WINDOW.json.
Build role mei:~/w/s1 (Skylake-SP). Scratch on mei: `~/work/LAYOUT-FIRST-WINDOW/` (fw-probe.sh, diag-<base|head>-<profile>/
with map, nm, readelf, htp.log per profile).

## Decisions in force

- Pending record `records/pending/LAYOUT-FIRST-WINDOW-1.toml` (series D-X; the coordinator assigns the id). Extends
  D-X.63 (clusters, `@place end`) and D-X.77; read D-X.73/74 for the data side of the start-up layout.
- `crates/libc-so/hot-text.first-window` (hand-kept): keys `cargo xtask so` links right after the start-up tier.
  Holds `liella_sys_fstat`. Stale lines fail `cargo xtask static` and `so`; a `--hottext-regen` build reports them.
- Contract: `init_startup_group_ends_inside_the_first_fault_around_window` (hot_text_products) fails with
  `FIRST-WINDOW margin` or `FIRST-WINDOW liella_sys_fstat` naming the tool, locale, localtime, localtime_mktime probes.

## Measured (hot_text_products, every phase, min of 5 runs; base = next a5636ce6b, jobs lfw-probe-base / lfw-probe-head)

| probe | release next -> head | fast next -> head |
|---|---|---|
| tool | 54 -> 53 | 52 = 52 |
| locale | 51 -> 50 | 49 = 49 |
| localtime | 18 -> 17 | 19 -> 18 |
| localtime_mktime | 18 -> 17 | 19 -> 18 |
| true, fork, float, mktime, strings, rebind | equal | equal |

The brief's figures (tool 53 vs 52) are from another host; not measured there. The only layout change is fstat's place
(release 0x418040 -> 0x417880, fast 0x4184c0 -> 0x417c00); no per-fault map in this run (the base map step failed:
dash has no `exec -a`), so the page attribution rests on lhis4-fmap3.
Margins: release 1908 bytes (fstat 0x417880..0x41788c), fast 1012 bytes (0x417c00..0x417c0c), window 0x408000..0x418000.

## Open / INFORMATIONAL

- Fast margin is 1 KiB: the next start-up growth over 1012 bytes fails FIRST-WINDOW. Then shrink the start-up tier
  (the printf `to_string` body is 9.8 KiB) or move rarely-first keys; never widen the window.
- `puts`/`puts_general` and untraced `extra -` keys (`liella_hot_equal_tail`, MutexGuard drop, `list_insert`) still
  sit at the head after the start-up tier; candidates for the next growth.
- The .text segment starts at 0x408000 (a 32 KiB cluster alignment); 64 KiB would make later windows full, not the
  first one larger. Not measured.

## Exact next step

Coordinator: assign the D-X id; land after lfw-gate2 and lfw-plants2 are green.
