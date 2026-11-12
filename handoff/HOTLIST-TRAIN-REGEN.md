# HOTLIST-TRAIN-REGEN: train-owned start-up lists. Stopped at the brief's cost gate (INCOMPLETE)

State: branch `task/hotlist-train-regen`, cut from origin/next 9b2a0c029. The only commits are this note and the
report record. No code was changed, no list was regenerated in the tree, and no D-X record was written (the
design is not settled; see the proposal). The brief's step 1 says to stop if one full regen costs more than about
15 min on mei. It costs 19 to 23 min there and 29 min on a 7-core slot, so this run stops with the numbers, the
inventory and a proposal.

## 1. Inventory: files under crates/libc-so/ that a regen touches or depends on
| File | Class | Written by | Notes |
|---|---|---|---|
| hot-text.list | MIXED (generated, merge mode) | `tools/hottext/regen.sh` via `merge.py`, once per profile; also `cargo xtask hottext-repair` and `tools/hottext/prune.py` | merge mode: the tracked list is an input to its own regen (old keys keep their place and their other-profile column). 523 traced keys plus 45 bare keys (tier `extra`, profile `-`, "kept unguarded until a trace hits it"): those 45 are the hand-kept part (21 of them also appear in hot-text.absent). Split: move them to a hand-written seeds file that merge.py reads, so the list becomes pure output |
| startup-text.keys | GENERATED (merge mode) | regen.sh (`page_faults true` under gdb) | |
| probe-text.keys | GENERATED (merge mode) | `probe_text.sh` (2 page_faults modes) | |
| startup-data.keys | GENERATED (merge mode) | `startup_data.sh` and `startup_data.py` (gdb, and perf page-fault addresses) | not shown to be deterministic, see section 2 |
| hot-text.absent | MIXED | by hand (the reason column) and `cargo xtask hottext-repair` | coupled to the list: `so` fails on a stale line. Split: the regen can derive the generated lines (keys one profile's archive lacks); the hand-written reasons stay as input |
| hot-text.clusters | HAND-WRITTEN input | by hand | names the first and last key of each cluster in hot-text.list. A regen that drops a boundary key breaks it |
| startup-state.list | HAND-WRITTEN input | by hand ("never generated", D-X.64) | stays on branches, as the brief says |

## 2. Cost of one full regen: both profiles, merge mode (#4839, `regen.sh . --build --allow-drop` per profile)
- **mei** (Skylake-SP, 48 threads, a loaded host). The same command ran there today in LAYOUT-HOTKEY-INLINE-STABLE-2's
  jobs (not mine; read from mei's `~/logs/jobs.tsv`): `lhis2-regen` took **1376 s (22.9 min)**, rc 0, and `lhis2-regen3`
  took **1161 s (19.4 min)**, rc 0. A third run, `lhis2-regen2`, failed (rc 1) at 531 s.
- **shiki** (Haswell-EP, one 7-core slot, idle host), on next 9b2a0c029, job `hotlist-train-regen-cost`: **1721 s
  (28.7 min)**, rc 0. Split by phase (seconds):

| profile | product build | traces + merges (15 gdb traces / probe_text / startup_data) | check build | total |
|---|---|---|---|---|
| fast | 281 | 283 (242 / 35 / 5) | 288 | 852 |
| release | 398 | 85 (71 / 10 / 3) | 386 | 869 |

  Product builds take 1353 s (79%) and traces 368 s (21%).
- **What it builds.** Each profile runs a full `cargo xtask so --hottext-regen` (libc.a and libc.so.6, libm, ld.so, then
  the layout check). It compiles 15 probe programs with `scripts/liella-cc` and links `page_faults.c`, traces them
  under gdb one after another, then runs probe_text.sh and startup_data.sh. Last comes a full `cargo xtask so` to
  check the rewritten list against the dead-key rule. That makes four complete product builds per regen.
- **A `so` with nothing changed costs as much as one after a list change.** Jobs `hotlist-train-regen-so-cost` and
  `hotlist-train-regen-so-lines` measured release at 386 s and fast at 284 s (282 s in the second job), against 292 s
  for fast after a list change. Cargo rebuilt nothing (its fingerprint log names no dirty unit), and no file under
  crates/, csys/ or include/ changed. Where the 282 s of a fast no-change `so` goes: libc.a and libc.so.6 are done at
  83 s (the hot-text grouping and its dead-key check at 56 s), libm at 115 s, ld.so at 156 s, and **the layout
  contract check takes the last 126 s (45%)**. The build tag hashes crates/ including the lists, so a list change
  does change `LIELLA_BUILD_TAG`, but it measured only +8 s. It is not the cost.
- **Output compared with next.** hot-text.list and startup-text.keys came out byte-identical. probe-text.keys lost
  one key (a `TscGate::fresh` closure). startup-data.keys differs on 26 lines: the fast profile gained written
  objects such as `_IO_2_1_stderr_` and `_IO_list_all`. Either next's data keys are stale, or the data trace depends
  on the host or the run. "Same tree gives byte-identical files" is **not** established for startup-data.keys.

## 3. Proposal (RULING NEEDED)
Do not add today's regen to every train. First make it cheap, in a story of its own, then resume this one:
1. **Regen build: libc.a only.** The traces link only libc.a and liella-crt1.o, so the regen's product build can skip
   libm, ld.so and the layout check. Measured step times put this at about 83 s instead of 284 s (fast), plus compile
   time after code changes.
2. **Check without a second full `so`.** The dead-key check is the grouping step (56 s into libc). Rerun only that
   step on the rewritten list, not the whole `so`. On the train, the gate's own `so` builds the final products anyway.
3. **Traces in parallel inside the slot.** The 15 gdb traces and the probe_text modes are independent; merge them in
   the fixed probe order so the output does not change. Estimate: about 40 to 60 s for fast (slowest single trace
   about 20 s) and about 15 to 20 s for release, instead of 283 s and 85 s.
4. With 1 to 3, the estimate (from the measured step times, **not measured end to end**) is about 3 min per profile,
   6 to 7 min for both, on a 7-core slot, against 28.7 min now. Measure it on mei. If both profiles still exceed
   15 min there, run fast on every train (the test products are Fast unless `LIELLA_FULL_PROFILE=1`) and release
   on a schedule (merge mode keeps the release column).
5. **Determinism first.** Before the train owns startup-data.keys, run the data trace twice on one host and once on
   mei and compare the outputs.

Files that would become train-owned after the split: the generated hot-text.list, startup-text.keys,
probe-text.keys, startup-data.keys (once 5 holds) and the derived part of hot-text.absent. Files that stay on
branches: startup-state.list, hot-text.clusters, the seeds file and the hand-written part of hot-text.absent. The
train command (`cargo xtask hot-lists --in-place`) does not exist yet; it is step 2 of this brief.

## 4. Other notes
- `hot-text.unpinned` (LAYOUT-HOTKEY-INLINE-STABLE, named in the brief) is not on next 9b2a0c029.
- The measurement left regenerated lists in role sh1's crates/libc-so (startup-data.keys and probe-text.keys differ
  from the tracked copies) and fast products built from them in target/liella. Re-sync the role before using it again.
- Job scripts: shiki `~/work/hotlist-train-regen/{time-regen,time-so,time-so-lines}.sh`. Logs: `~/logs/<label>.log`.
  All three jobs ended rc 0. Nothing is left running.

Next step: a ruling on section 3. Then a story for items 1 to 3 (measured on mei, with plants on the new check
path), then resume this brief at step 2. Plan rows to read: D-X.49, D-X.50, D-X.62, D-X.63 (the lists' contracts),
D-X.66 (the codemap train-owned pattern this follows).
