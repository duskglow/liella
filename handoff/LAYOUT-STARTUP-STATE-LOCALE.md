# LAYOUT-STARTUP-STATE-LOCALE: deterministic start-up state order (#4923), setlocale's objects (#4915), first-call cluster (#4942)

**State (2026-11-12, run -3): INCOMPLETE (context checkpoint), do not land yet.** Branch
`task/layout-startup-state-locale-3` (= -2's head dfaceca76 + 3 commits; no merge). Role mei:~/w/s1. Scratch on mei:
`~/work/LAYOUT-STARTUP-STATE-LOCALE-3/` (`branch.sh` = -2's for this role: xtask unit tests, both profiles' products +
the three hot_text_products order/access tests + every probe at every pad against next (`pads.py`); saved products
`next-{fast,release}` and `b1-{fast,release}` (= -2's head, copied from kanon), branch products land in `b2-*`).

## Decisions in force
Ruling #4923 (order), #4942 (first-call tiers in a second cluster outside the every-process run; #4943 relocation
sections move with targets: done in -2), D-X.70, D-X.64, D-X.69, jitter rule #4347. No decision record yet for the
order rule or the first-call cluster (both want one: `records new decision D-NEW --series D-X`).

## Done (runs -1, -2: see their commits; run -3 below)
- -1/-2: list-order pass (`order_sections`, relocation sections follow), the rename after the copies pass's `ld -r`
  join, the access check over all ten probe modes, the order test. -2 measured: locale/tool flat and 2-4 lower than
  next at every pad, `true` +1 at every pad (first-call state between the base tier and the C words).
- -3 (commits 8ac3454d0, cfd0da5bc, f1047eb5c), ruling #4942:
  - `startup-state.list`: a `cluster first-call` line; every entry after it is first-call state (the setlocale tiers
    incl. MAPS..SECURE and `_nl_msg_cat_cntr`, the LOCALE block, the time tiers, setenv's table, the catalog tier).
    NSCACHE (every process) moved up to the every-process locale block.
  - `coldbss.rs` `state_plan`: the zero-filled first-call sections of the member with the most such bytes (the Rust
    object) are renamed `.lbss` (exact name; cold tables are `.lbss.<sym>`), ordered after the every-process tier and
    ahead of that member's cold tables in the section table, the first one aligned to 4096 (`objcopy
    --set-section-alignment` before the renames; the build checks exactly one `.lbss` section has it). Initialised
    first-call objects (stdout, `_IO_2_1_stdout_`, intl GENERATION/SETTINGS/MEMO, NEXT_ID, ENV_VALUES/REPLACED,
    ~300 bytes) and another member's (`csys_7.o`: `__daylight`, `__timezone`, `__tzname`) stay in `.data1`.
  - Why `.lbss`, not the ruling's `.ldata` example: `.ldata` starts its own PT_LOAD (GNU ld default script) and the
    static-PIE start accepts exactly one writable segment (D-X.68). `.lbss` follows `.bss` in that segment.
  - hot_text_products: the order test covers both clusters (per cluster and object) and asserts the cluster starts a
    page; the access check names any first-call cluster object the `true` trace touches.
  - Unit test in `declared_startup_state_is_found_whatever_its_symbol_suffixes` covers the split and order.

## NOT VERIFIED (the checkpoint came first)
Whether f1047eb5c compiles and its tests pass, and every count: job **lssl3-branch2** (mei) runs them now; read
`~/logs/lssl3-branch2.log` (`grep -E "test result|FAILED|first-call|WORSE|sum over" `). Expectation: `true` back to
next's level (`.data1` shrinks by the zero-filled first-call bytes); locale/tool may pay 1 page for the cluster
(they touched those objects on base-tier pages before): compare with -2's 48/49 and 51/52 (next 50/51, 54/55).

## Exact next step
1. Read lssl3-branch2. If it failed to compile or a test is red, fix that first (clippy + static on mei).
2. If `true` <= next everywhere and nothing WORSE: gate (`affected --run --base origin/next`), hot_text_products,
   plants (`handoff/LAYOUT-STARTUP-STATE-LOCALE-plants.txt`: 7 plants, 3 new for -3), clippy, static, hand back.
3. If locale/tool lose -2's gain: see which first-call page they now fault (faultmap.py on kanon scratch of -2) and
   put the objects locale touches first in the cluster; never raise a bound.

## Jobs (mei)
lssl3-lint0 (inherited head dfaceca76: clippy + static + products, rc 0); lssl3-branch1 (8ac3454d0) and lssl3-unit1
(cfd0da5bc): compile errors, superseded; lssl3-branch2 (f1047eb5c): running at hand-back.
