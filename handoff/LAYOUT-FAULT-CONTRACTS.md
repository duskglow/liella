# LAYOUT-FAULT-CONTRACTS (brief C of LAYOUT-STABILITY-RETHINK): merged with next, perf fault-class contracts in; phase test, all-mode regen and #4767 left

**State (2026-11-11, run -2):** branch `task/layout-fault-contracts-2` = run 1 (ada298b65) + merge of origin/next
c0b379b7e (LAYOUT-TEXT-PIN-11 landed; 6e3102d70) + 69c4da55e (fault-class contracts) + b92b72bd4 (release bounds,
plants file). Role mei:~/w/s3 (Zen 1). Decisions in force: D-X.65 (this story, parts 1-4), D-X.61, D-X.63, D-X.56,
D-X.49, D-X.47; rulings #4349, #4593-#4597, #4767. Design: handoff/layout-stability-rethink.md sections 3-4.

## Done (run 1)
Merge of HOTTEXT-REGEN-HOSTS-2 (per-profile lists `hot-text-<profile>.list`, #4593); the regen build reports a broken
contract and goes on (`layout::regen_verdict`), the plain `so` enforces; the hermetic trace (`env -i`, LANG only,
TZ only where a probe sets it #4596, ASLR off, fixed /tmp path, `python3 -B`, `bash regen.sh`, merge.py host class
#4597). perf `-e page-faults -d` works for `claude` on mei, keke, shiki and kanon (#4595).

## Done (run 2)
1. Step 0: clippy + static on the inherited head green (lfc2-lint0).
2. **Merge of origin/next.** Conflicts: next's regenerated single `hot-text.list` (PIN-11, both profiles, Haswell-EP)
   split into the two per-profile lists by the D-X.61 rule with run 1's script (keeps the host-class pairs; next's
   three dropped keys, incl. `rt::start::program`, stay dropped); `startup-text.keys` = next's keys and order + our
   pairs column; `startup_data.sh` = next's isolated link + `python3 -B`; next's new `cargo xtask hottext-repair`
   now repairs each profile's list against that profile's matched keys. `hot-text.absent`: next's release line for
   `time::zone_path` removed (with per-profile lists the release list does not hold the key, so release `so` called
   it stale).
3. **Brief item 1, perf contracts.** Kind `fault-classes` (xtask/src/layout.rs, FAULT-CLASSES section): links
   page_faults.c, runs `setarch -R perf record -e page-faults -c 1 -d` once per row (env: LANG + fixed PATH, stdin
   /dev/null, cwd /), classes each address by `readelf -lW`: `text` (exec segment, 64 KiB window holding hot-group
   text), `text-cold`, `ro`, `relro`, `written`, `other` (unbounded). Over a bound: the class and `symbol+off` at
   each of its faults. perf unavailable: NOT MEASURED (an error under CI). Rows and bounds (this head, Zen 1):

   | row (mode) | fast text/cold/ro/relro/written (other) | release |
   |---|---|---|
   | STARTUP-FAULT-CLASSES (true) | 2/0/3/1/5 (8) | 2/0/3/1/5 (8) |
   | LOCALE-FAULT-CLASSES (locale) | 5/3/3/1/8 (30) | 6/2/4/1/8 (30) |
   | TIME-FAULT-CLASSES (localtime_mktime) | 9/6/3/1/6 (25) | 6/2/3/1/6 (25) |

   Test: `layout::tests::fault_addresses_are_classed_by_segment_and_a_rise_names_the_symbol` (no spec clause).
   Plants: handoff/LAYOUT-FAULT-CONTRACTS-2-plants.txt (results appended there).

## Not done (next step, in order)
1. **Phase test (item 2):** extend `init_zero_fault_calls_hold_at_every_text_phase` (hot_text_products.rs, pads
   0x800/0x2000/0x8000/0xF000, pre and cold) to require identical per-class counts. One shared classifier is needed
   outside xtask (the test crate cannot call xtask): port `parse_segments`/`fault_class` to a small script or a test
   support module. The non-zero `text-cold` counts of locale/time (3 and 6 fast) are the first thing it will judge.
2. Make STARTUP-TEXT-WINDOWS / STARTUP-DATA-WINDOWS report-only where perf runs (key lists = placement input only);
   left enforced in this run (no bound loosened).
3. **Regen item 2:** trace every page_faults mode (getrusage's real tier, #4327).
4. **#4767:** the hermetic regen must not depend on the trace host's CPU dispatch (trace every dispatch path or key
   variants together) and must keep a key whose function still exists (inlined call site); a test for each.
5. Regenerate both lists on the merged tree on two host classes.

## Not to redo
Run 1's merge and hermetic changes; this run's merge with next and its list split; the perf host survey; the bounds
above (re-measure only after a layout change).
