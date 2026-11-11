# LAYOUT-DATA-CLUSTERS-3: tzname/NEXT_ID into .data1, size-aligned relro, start-up data keyed by access

**State (2026-11-11):** branch `task/layout-data-clusters-3` (on `task/layout-data-clusters-2`; origin/next is not an
ancestor). Code 6cd1d1e0f + c30af27bf (unit-test fixtures, D-X.67 record). Role shiki:~/w/sh1. Decision: D-X.67
(records/decisions/D-X/D-X.67.toml; extends D-X.64). Earlier: `handoff/LAYOUT-DATA-CLUSTERS-2.md`.

## -2's red, attributed before any change
-2's gate ldc2-gate (mei) failed only on the localtime bounds (fast 21 > 20); its plants passed. Fault map on
shiki (Haswell-EP), next's products (aad3df74d, copied from mei) vs -2's head, both profiles, same host, scratch
`ldc_measure.py` + `chrono.sh` (perf page-faults -d, chronological, with ip):
- **fast localtime 21 vs 20** (the localtime_r window): text 4 = 4 (next: localtime_r, Cursor::number,
  finish_grow, Zone::at_cached; head: localtime_r, sys_fstat, finish_grow, zone::local_in), heap/mmap 14 = 14,
  vdso 1 = 1, data NEXT_ID 1 = 1, and **`__tzname` +1**: next's `.data` ended with `__tzname` (0x851be0), which
  shared its page with `.data1` (0x851bf8); -2's 16 KiB-aligned cluster (0x874000) left it alone on page 0x871000.
- **release tool/locale Dr 2 vs 1**: `.data.rel.ro` (28 KiB) at 0x7af1a0..0x7b5fc8 straddles the 64 KiB
  boundary 0x7b0000; tool reads relro at 0x7af290 (ip 0x412b5d) and 0x7b2b98 (ip 0x42a7b1). On next it lay
  inside one window (0x792180..0x798fc0). A phase effect of the text/rodata sizes, not of -2's data moves.
- Also seen: release `strings` mode 1 on -2's head vs 0 on next (INFORMATIONAL, not attributed; fast 0).

## What changed (D-X.67)
1. `xtask/src/coldbss.rs`: `time::zone::NEXT_ID` (suffix) and `.data.rel.local.__tzname` (C section by name)
   join `.data1` (both written by the first zone load).
2. `csys/src/machine/entry.S` + `startup_ro::cluster_aligns`: `.data.rel.ro` aligned to pow2(relro bytes over
   the archive), 4..64 KiB (`LIELLA_RELRO_ALIGN`), as `.rodata1`/`.data1` are.
3. `tools/hottext/startup_data.{sh,py}`: a second probe link with `SUBALIGN(65536)` on `.data`, `.data1`, `.bss`,
   `.lbss` (from `ld --verbose`), traced with perf: every writable object `true` reads or writes is keyed
   (#4602). **The keys file is NOT regenerated yet** (needs `tools/hottext/regen.sh` on both profiles).

## Measured (shiki, job ldc3-after-2: base = next aad3df74d products, head2 = c30af27bf, orig link, minflt)
| products | true | locale | tool | fork | localtime | lt+mktime | float | strings |
|---|---|---|---|---|---|---|---|---|
| fast next | 17 | 55 | 58 | 7 | 20 | 20 | 3 | 0 |
| fast -2 (ldc3-map) | 12 | 51 | 54 | 6 | 21 | 21 | 3 | 0 |
| fast this | 12 | 51 | 54 | 6 | **19** | **19** | 3 | 0 |
| release next | 15 | 54 | 57 | 6 | 19 | 19 | 4 | 0 |
| release -2 (ldc3-map) | 11 | 53 | 56 | 6 | 19 | 19 | 3 | 1 |
| release this | 11 | **52** | **55** | 6 | **17** | **17** | 3 | 1 |
Cluster alignments: fast rodata1/data1/relro 65536/16384/65536, release 65536/16384/32768. Pads (cold/pre/lbss)
not re-measured in this run: the gate's `init_data_clusters_hold_at_every_text_phase` checks them.

## Open / next step
1. Run `bash tools/hottext/startup_data.sh <tree>` for fast and release products (or regen.sh), commit the new
   `crates/libc-so/startup-data.keys`, rebuild, re-measure (expect `__libc_single_threaded`, `_IO_2_1_stdout_`
   and stdout keyed and moved into `.data1`; watch `.data1` page count and the STARTUP-DATA-WINDOWS budget).
2. The .data-order test (item 2): add region `data` (pad section `.data.liella_pad` in the padded start file,
   0x800/0x2000) to `init_data_clusters_hold_at_every_text_phase`, and, where perf records, assert no
   true/tool/fork fault lands in `.data`/`.bss` outside `.data1` (naming the symbol); plant: drop the
   `_IO_2_1_stdout_` key line, msg: the symbol name. Not done in this run (budget).
3. Bounds untouched (item 3); per-probe numbers below for the owner's re-derivation (#4348).

## Jobs (shiki)
ldc3-map (fault map, rc 0), ldc3-unit-2 (9 pass), ldc3-after-2 (rc 0), ldc3-lint-2 and ldc3-gate-2 (chained, running at
hand-back; chain ~/work/LAYOUT-DATA-CLUSTERS-3/chain.sh). Superseded: ldc3-unit (rc 101, fixtures before c30af27bf),
ldc3-probe (a `true` label-reuse probe, not evidence). No plants run (item 2 test not written).
