# LAYOUT-TEXT-PIN-10 (brief A, continuation 10): dead keys gone; merged head over the tool/locale bounds by 1

**State (2026-11-11):** branch `task/layout-text-pin-10` = task/layout-text-pin-9 81c825fdf + a clean merge of
origin/next aad3df74d (no conflicts) + the regenerated hot-text.list (e11011ea1) + this note and the report.
Role shiki:~/w/sh2 (Haswell-EP). Decisions in force: D-X.63, D-X.56 (regen), D-X.47 (tiers), D-X.49, D-4.53;
rulings #4539, #4541. Earlier notes: handoff/LAYOUT-TEXT-PIN-9.md ... LAYOUT-TEXT-PIN.md.
**Verdict: INCOMPLETE.** The fault bounds read over on the merged head. As the brief says, no bound was changed.

## Done this run
1. Step 0: clippy and static on the inherited head 81c825fdf were green (job layout-text-pin-10-lint0).
2. Merged origin/next aad3df74d (25 commits; clean). Next did not touch tools/hottext or crates/libc-so lists.
3. Regen of both profiles on the merged head:
   - Run 1 (layout-text-pin-10-regen-both, rc 1, by design): regen.sh REFUSED to drop the two CI-red keys
     `liella::locale::find_cached` / `attach_charmap` over `<global_locale::Files, boundary::locale::Glue>`,
     tier locale, fast. The #4539 guard did its job.
   - Cause: in the merged fast libc.a, `nm` and `readelf -SW` show no symbol or section for either function. Next
     inlines them into their callers now. So the keys are really gone, not renamed.
   - Run 2 (layout-text-pin-10-regen-both2): fast regen ran with `--allow-drop`. Its script exits 3 unless the
     drop list is exactly those two keys, and it was. The release regen ran without `--allow-drop` and dropped
     nothing. `cargo xtask so` (fast) passed the dead-key check.
   - The whole list change is: the 2 keys dropped, and `_RNvNtC6liella6locale7builtin` now tiered
     `fast,release` (it was `release`). The other four generated files are byte-identical.
4. hot_text_products: 16 passed, 2 failed (the bounds below).

## Measurements (fast products, hot_text_products, shiki, same host, same procedure)
| tree | start-up (bound 17) | setlocale + printf (bound 55) | coreutils-shaped (bound 58) | fork child (bound 7) |
|---|---|---|---|---|
| origin/next aad3df74d, RAM copy (job layout-text-pin-10-next-ref, 14/14 pass) | 17 | 55 | 58 | 7 |
| merged head after both regens (job layout-text-pin-10-regen-both2) | 16 | **56** | **59** | 6 |
**Against next on the same host: tool +1, locale +1, start-up −1, fork −1.** At -9 (next a43eb7c3c) the gap
was +2 / +2 / −1. Since then next itself has risen from 56/53/16 to 58/55/17 on this host, and is now at all its
bounds. The list change here is two keys (inlined away) and one tier gain. So the +1/+1 most likely comes from
the branch's layout code (D-X.63 cluster alignment / cold renames), as -9 suspected. That is not diagnosed:
the brief's scope excludes it.

## Open (next step, in order)
1. Fault-map the merged head against next aad3df74d on one host (-8's fmap.sh / classify.py,
   ~/work/LAYOUT-TEXT-PIN-8 on keke; tools/hottext/faultmap.py). Find which 64 KiB windows hold the one extra
   tool fault and the one extra locale fault. Suspect: cluster padding that pushes a tool/locale-tier key into
   another window.
2. Per ruling #4347 (layout jitter), the coordinator decides whether this chain holds or a fault-map story is cut.
   Next is now at all its bounds on this host, so any growth will red next too.

## Not to redo
The merge with next aad3df74d. The two-profile regen and its drop check. The same-host next reference above.
