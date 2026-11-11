# LAYOUT-STARTUP-RODATA(-2..-8): hot read-only data in one 64 KiB-aligned .rodata1 (D-X.62, amends D-X.50)

**State (2026-11-11, run -8): COMPLETE, ready to land.** Branch `task/layout-startup-rodata-8` = run 7's branch +
origin/next 07a4db6ac (LAYOUT-DATA-CLUSTERS-4C merged, the hold of ruling #4347/#4483 lifted) + plants file + this
handoff. Build role kanon:~/w/kn1 (Broadwell-EP). Scratch: kanon ~/work/LAYOUT-STARTUP-RODATA-8/ (run1.sh, measure7.py =
run 7's, an.py = fault map of the last fork child), results /mnt/zram/claude/LAYOUT-STARTUP-RODATA-8/m1/results.jsonl.

## Run 8: merged next; fork child back under its bound (no bound change)
- **What the branch still carries over next:** the D-X.62 work (runs 1-4) had already landed on next. After the merge
  the only code delta is one test assertion in `crates/rt/src/start.rs` (`program_headers_give_the_tls_image_and_the_bias`:
  two `PT_TLS` headers, the first counts). Merge conflicts: `start.rs` takes next's program-header loop
  (LAYOUT-DATA-CLUSTERS-4C, D-X.64), which solves the same per-header cost run 5 solved (run 5's loop dropped; D-X.62
  restored to next's text, so it no longer describes the replaced loop); `docs/codemap/symbols.tsv` = next's.
- **Fork child (fast products, the test's own harness; job layout-startup-rodata-8-forkmeasure):**
  `cargo test -p liella --test hot_text_products init_fork_child`: **6** (warm-up 6, runs [6,6,6,6,6]; bound 7).
  measure7.py pads (min of 5 after warm-up): `cold` 0..0x10000: 6 at every pad; `pre`: 6, except 7 at
  0x2000-0x5000 and 0xd000. `true` 12-13 everywhere (run 7 on shiki: 16-17). **<= 7 everywhere.**
- **Fault map (perf, last child):** the `timer::EPOLL` read that cost run 7 its 8th fault is gone; the only `.data`
  fault is the `timer::WAKE` write, at every layout. The `pre` 7s are text: a `set_robust_list` text page
  (0x2000-0x5000) or `fork::child_reset` crossing a page boundary (0xd000). That is hot-text placement
  (LAYOUT-TEXT-PIN), not this story's data.
- Lint (workspace clippy all targets/features, `cargo xtask clippy`, `cargo xtask static`): clean on the merged head
  (layout-startup-rodata-8-lint). Plants 1/1 CAUGHT (layout-startup-rodata-8-plants). Direct-tier gate
  (`cargo xtask affected --run --base origin/next`, layout-startup-rodata-8-gate): rc 0, 7 suites, 4481 passed /
  0 failed / 450 ignored, conformance 17/17, peak 12G.
- FORK_BOUND not tightened: the own layout is 6, but `pre` pads still reach 7 (text jitter, above).

## Earlier runs (condensed)
- Runs 1-4 (landed): `xtask/src/startup_ro.rs` renames start-up read-only sections to `.rodata1` (starts =
  startup-text.keys + `HOT_TIERS` hot keys + `probe-text.keys`, transitive closure, whole names from the ELF members);
  entry.S aligns `.rodata1` to 64 KiB; STARTUP-DATA-WINDOWS ro budgets 1/1; `cargo xtask startup-ro-plan`.
- Run 5: startup_instr_c 2159 > 2150 from the extra R `PT_LOAD`; fixed in `start::program` (now superseded by next's loop).
- Run 6: #4470 cost of the extra R segment per start: +1 PT_LOAD, +1 minor fault, ~+9.2k kernel instructions (Zen 1).
- Run 7: fork child 8 on shiki was the RW data phase (timer::EPOLL read), held for LAYOUT-DATA-CLUSTERS (#4483).

## Open / next step
1. Land (the train regenerates the codemap views).
2. Optional follow-ups: tighten FORK_BOUND to 6 once LAYOUT-TEXT-PIN removes the `pre` text jitter; the `.data1` half
   (#4315) and the bounds it would let tighten (tool / locale); fast localtime at its bound 20.
3. Run 4's lsr-reloc-not-followed plant printed WARN-NO-MSG (caught by the products build): its msg should accept that.

Not to redo: 16 KiB alignment (bound 33 KiB fast), "all tiers" (160 KiB), symbol-only matching, hunting the straggler,
measuring next's release localtime (19), run 5's header loop (next's D-X.64 loop covers it).
Group headroom: fast 60064 of 65536 bytes; a new probe mode's data may not fit (the archive-wide check fails `so`).
Plan rows: D-X.50, D-X.62, D-X.64 (records/decisions/D-X/), D-X.48/49 (hot text), contract STARTUP-DATA-WINDOWS.
