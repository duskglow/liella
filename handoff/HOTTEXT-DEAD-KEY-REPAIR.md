# HOTTEXT-DEAD-KEY-REPAIR: handoff

State: branch `task/hottext-dead-key-repair`, cut from next 672acd9aa704 (next is an ancestor: the branch is
a "next merged in" tree). HEAD: see `handoff/reports/HOTTEXT-DEAD-KEY-REPAIR.json`.

## What landed

`cargo xtask hottext-repair [--at <rev>]` (xtask/src/hottext.rs `repair_cmd`, `repair_lists`, `check_lists`;
row in xtask/src/main.rs's command table):

1. Builds both profiles' processed archives (`so::build_archive`, the archive step of `cargo xtask so`; fast then
   release) with the dead-key check reported, not enforced (the in-process `REGEN` switch of `--hottext-regen`,
   not the flag: the CI refusal of the flag does not apply, because step 4 enforces the check on the result).
   `target/liella` is not touched; the weakened archive goes to `target/product/<profile>/hottext-repair-libc.a`.
   Each `target/product/<profile>/hot-text.matched` is removed first, so a stale one is never read.
2. `repair_lists` on `crates/libc-so/hot-text.list` and `hot-text.absent`:
   - a key no profile matches: line dropped (prune.py's rule, in Rust so it is unit-tested);
   - a key a profile does not match, where the contract does not already allow it (trace names only the other
     profile, or an absent line exists): absent line `<key>\t<profile>\tinlined in <profile> at <rev> (cargo xtask
     hottext-repair)` appended;
   - an absent line the contract calls stale (key matches again in its profile, or no longer listed, e.g. just
     dropped): removed (the dead-key check refuses stale lines, so the repair must, too);
   - never adds a key, never reorders; all other lines byte-identical.
3. Prints every change and a summary; `hottext-repair: nothing dead (no file changed)` when clean.
4. Re-runs the dead-key contract (`unmatched`/`unmatched_error`) for both profiles on the new texts; fails with
   the check's message if it still fails. `--at` default: `git rev-parse --short=12 origin/next` (role dirs are not
   git trees: pass `--at`).

## Measured (shiki role sh1, Skylake-SP class, warm target/ from earlier jobs)

Run on the branch tree (= next 672acd9aa704 + this change): rc 0, `nothing dead`, **226.7 s wall** for the two
archive builds (job `hottext-dead-key-repair-run`). A cold target/ will be slower (fat-LTO release dominates).

## Tests / plants

- `hottext::tests::repair_drops_dead_keys_records_one_profile_inlining_and_keeps_live_keys` (no spec clause: build
  tooling): fixture list with a dead key, a key inlined in release only, a fast-only-traced key, an already-allowed
  key, a key-only (pre-tier) line, a stale absent line and an absent line of a dropped key; checks the new texts,
  the change lists, that the check fails before and passes after, and that a second run changes nothing.
- Plants: handoff/HOTTEXT-DEAD-KEY-REPAIR-plants.txt (drops a live key; records an allowed key; keeps a stale line):
  3 CAUGHT, 0 WARN (job hottext-dead-key-repair-plants2). The first run (hottext-dead-key-repair-plants) caught all
  three but WARN-NO-MSG: the panic line carries the thread id, `'<test>' (<tid>) panicked at`; regex fixed.
- Jobs (shiki): hottext-dead-key-repair-unit, -clippy, -static, -run, -plants2 rc 0. Gate
  hottext-dead-key-repair-gate (`affected --run --base origin/next`) ESCALATED TO FULL by the tool (any
  xtask/src change: "its commands are part of the gate"), so it runs the whole `cargo xtask test`; left running
  at hand-back for the dispatcher (PENDING JOBS).

## Not done / next step

- The coordinator wires it into liella-ops tools/merge_next.sh (red only on dead keys -> `cargo xtask
  hottext-repair --at <next sha>`, commit the two files). Not done here (ops repo is read-only for agents).
- No decision record (none reserved by the brief); the behaviour is described here and in the doc comments.
- Do not redo: per-profile lists are NOT on next (one hot-text.list with a profiles column + hot-text.absent);
  the repair follows that layout.
