# LAYOUT-DATA-CLUSTERS-11: ldc9-locale-set diagnosed; `_nl_msg_cat_cntr` declared, LOCALE_BOUND 52 -> 51

**State (2026-11-11):** branch `task/layout-data-clusters-11` = -10's `1b95efda9` + `527fa6c49` (`symbol
_nl_msg_cat_cntr` in `crates/libc-so/startup-state.list`) + `a2cce5f96` (LOCALE_BOUND 52 -> 51, plants file
`handoff/LAYOUT-DATA-CLUSTERS-11-plants.txt`) + this handoff. Role mei:~/w/s2 (Skylake-SP). Scratch on mei:
`~/work/LAYOUT-DATA-CLUSTERS-11/` (`diag.sh`, `m.sh`, `map.sh`, `plant.py`, maps `head-*`, `plant-*`, `fix*`).

## Decisions in force
D-X.70 (the declared list), D-X.64 (clusters), D-X.50. No new record: one entry added under D-X.70.

## Why ldc9-locale-set survived (job ldc11-diag, mei, fast products, 5 runs each, all identical)
| tree | true | locale | tool |
|---|---|---|---|
| -10 head 1b95efda9 | 11 | 51 | 54 |
| -10 head + plant (set dropped) | 11 | 52 | 55 |
| -11 (`_nl_msg_cat_cntr` declared) | 11 | **50** | 54 |
| -11 + plant | 11 | 52 | 55 |

The startup-data.keys do NOT place the set (no global_locale query/counter statics in the keys); the declared
entries still matter. Fault map (perf page-faults -d): on the -10 head the first setlocale's only write on the last
`.bss` page (0x87b, untouched by start-up) was `_nl_msg_cat_cntr` (`liella_boundary::data`, after next's merge):
one fault. With the plant, `NAME_COUNT` lands on that page: a read fault (zero page) and a write fault, and the
counter's fault is absorbed: net +1 (52), inside LOCALE_BOUND 52 (count plus 1). Before the merge the plant cost +2.
Fix: declare `_nl_msg_cat_cntr` (moves into `.data1`, whose pages start-up already faults): locale 51 -> 50,
LOCALE_BOUND -> 51 (ALWAYS FORWARD); the plant reads 52 and is caught. TOOL_BOUND stays 56 (the tool reads 54 on
the merged head; the -1 came with next, not this change; the brief said keep 56; see the report).

## Jobs (mei)
ldc11-lint0 (inherited head: xtask clippy + static, rc 0), ldc11-diag and ldc11-measure (probes, rc 0). Final head
a2cce5f96: ldc11-lint (workspace clippy -D warnings, xtask clippy, static), ldc11-gate (direct tier, --base
origin/next), ldc11-plants. Results: `handoff/reports/LAYOUT-DATA-CLUSTERS-11.json`.

## Plants
ldc9-locale-set (msg `> 51`); ldc11-msg-cat-cntr marked equivalent: dropping the new entry costs exactly one
fault (50 -> 51) and every fault bound keeps one fault of slack, so no bound test can see it. ldc8-time-compat
unchanged (equivalent, release-only level).

## Next step
LAYOUT-TEXT-PIN-14 (waits for this story). Open from -9: release-profile locale / tool counts (expect 50 / 54);
fold the two `.data1` list tests into one; extend `init_startup_data_accesses_are_in_the_data1_cluster` to trace
`relay locale` / `relay tool`. That extension would catch single-fault losses such as ldc11-msg-cat-cntr directly
(a written small static outside `.data1`), which the count bounds cannot.
