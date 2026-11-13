# LAYOUT-RETHINK: a start-up layout whose inputs cannot vanish under the compiler

**State (2026-11-13):** planning story, no product code. Branch `task/layout-rethink`, base b31418181 (next when
cut). Deliverables: `plan/layout-rethink.md` (frame, options, measurements, recommendation, migration, owner
questions) and the pending record `records/pending/LAYOUT-RETHINK-1.toml` (D-NEW, series D-X, proposed).

**Decision proposed (owner rules first):** option (e), the build traces itself. `cargo xtask so` traces the probes
against the archive it just built and groups exactly what they ran. No function name crosses a build. Stories
LT-1 (trace at build), LT-2 (clusters by probe phase), LT-3 (ptrace tracer), LT-4 (host-canonical trace), LT-5
(delete lists, absent, unpinned, dead-key check, regen flag, repair, recheck, train step, lane; review 45 pins),
LT-6 (optional, libc.so.6).

**Measured (shiki, Haswell-EP, relay harness, every run identical; plan section 3):**
- P1, a list traced from this build only: 21 of 22 probe rows equal or lower than next, both profiles; fast `tool`
  52 -> 53 (one text window, open, for LT-1). Re-trace byte-identical; second build's `libc.a` byte-identical;
  layout-check and hot_text_products pass, both profiles. Trace cost 59 s fast / 26 s release (gdb).
- Compiler change (inline threshold, fast): a stale list costs up to +8 (locale) / +7 (tool) / +5 (localtime) at
  200 (2 dead keys, 26 newly outlined), +3 at 300 (9 dead keys); fresh traces restore next's counts. The loss
  follows newly outlined functions, not dead keys. The hand-kept cluster file broke the fresh rebuild at 300.
- P2, exported-root closure: 2.1x traced bytes, 4x in the start-up tier, misses 61 sections reached indirectly; fails
  STARTUP-TEXT-WINDOWS / start-up read-only window. Rejected.

**Open questions (owner):** plan section 5: build-time tracing and its sandbox fallback; cross-host
reproducibility (LT-4 before the default changes); HOTLIST-TRAIN-REGEN-2 withdraw or interim; warning-only
dead-key check as interim; records schema has no `proposed` status (record is `active`, says Proposed).

**Do not redo:** the measurements above (scripts `~/work/LAYOUT-RETHINK/{count.py,closure.py,lists.py,proto1-3.sh}`
on shiki, results `~/work/LAYOUT-RETHINK/out-summary/`). PGO and BOLT were not prototyped (reasons in plan 2(b), 2(c)).

**Next step:** owner ruling on plan section 5, then brief LT-1 (and LT-4 if cross-host identity is required first).
Plan rows: D-X.20, D-X.47, D-X.53, D-X.56, D-X.63, D-X.77, D-X.78; `handoff/layout-stability-rethink.md`.

**Jobs (shiki):** lr-proto1, lr-proto2, lr-proto3 (prototype measurements, rc 0); the static job on the final head
is named in the report json. No code changed, so no gate or plants.
