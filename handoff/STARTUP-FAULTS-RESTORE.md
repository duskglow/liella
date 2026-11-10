# STARTUP-FAULTS-RESTORE: static start-up back to 61 page faults, and contracts that hold it

State: branch `task/startup-faults-restore-4` (continues `-3`, `-2`, `-1`), merged with origin/next 2bc03ede6
(coordinator-allowed; clean merge). Role shiki:~/w/sh1 (Haswell-EP). Decisions in force: D-X.20, D-X.22, D-X.32,
D-X.47, **D-X.53** (this story: bounds = count + at most 1, one per mode at the larger profile's count, lowered when
a count falls; a dead hot-text key fails `so` unless allow-listed with a reason; start-up state words in `.data1`;
counts taken through the relay harness). Plants: `handoff/STARTUP-FAULTS-RESTORE-plants.txt` (runs 1-3, 5 plants)
and `handoff/STARTUP-FAULTS-RESTORE-4-plants.txt` (run 4, 2 plants). Report: `handoff/reports/STARTUP-FAULTS-RESTORE-4.json`.

## What the story does (runs 1-3)
* **Dead keys fail the build** (`xtask/src/hottext.rs`, through next's `dead_key_verdict`): every key of
  `crates/libc-so/hot-text.list` must match a function section in a fast or release build, except a key traced only
  in the other profile or a line of `crates/libc-so/hot-text.absent`. `tools/hottext/prune.py` drops dead keys.
* **Start-up state on one page** (`xtask/src/coldbss.rs::move_startup_state`): listed statics (incl. NSCACHE) go to
  `.data1`, on the page where `.bss` starts; `scan.S` puts `scan_level` there.
* **Bounds** in `crates/core/tests/hot_text_products.rs`, structural tests `init_startup_path_lies_in_the_hot_text_range`
  and `init_startup_state_statics_are_in_data1`. `tools/hottext/faultmap.py`: faults -> section and symbol.

## Run 4: why CI was red, and the fix
* The coordinator note blamed the release profile; the CI log says otherwise: the gate built `so: profile fast` only,
  and both failures were in `tests-interposed` (the second `cargo xtask test` pass, exports interposed on the test
  process, `xtask/src/main.rs::workspace_tests`). Reproduced on shiki with the same fast products:
  plain pass 17 / 57 / 60, interposed pass 18 / 58 / 61 (CI's numbers). Cause: "true", "locale" and "tool" count the
  whole process (RUSAGE_SELF), which includes the spawned process's pre-exec work in the SPAWNER's code (host
  posix_spawn in one pass, Liella's in the other).
* Fix: `page_faults.c` mode `relay <mode>` vforks and execs itself with raw syscalls (one asm block), at its
  AT_EXECFN path (read from the auxv after envp) with the same env; the harness always passes `relay`. Two dead
  ends on the way (both measured, release 59 / 62): exec of `/proc/self/exe`, then libc `vfork`/`execve`/`waitpid`/
  `getauxval`; the cause of the +2 was the extra libc.a members moving the test program's layout, not the path.
* Counts, relay harness, Haswell-EP, every run identical, plain and interposed passes identical:
  | mode | fast | release | bound (was) |
  |---|---|---|---|
  | startup only | 16 | 15 | 17 (18) |
  | setlocale + printf | 56 | 56 | 57 |
  | tool | 59 | 59 | 60 |
  | fork child | 7 | 6 | 7 |
  | localtime, localtime+mktime | 19 | 19 | 20 |
  | float | 2 | 4 | 5 (6) |
* Host classes: before the relay, the same two binaries (fast, release) read identical counts on Haswell-EP,
  Skylake-SP (mei) and Zen 1 (keke) with a posix_spawn replica of the harness (`pf_launch.c`, kept in
  `~/work/STARTUP-FAULTS-RESTORE-4/` on those hosts; it matched the in-test counts exactly on shiki). The products
  remap the build path to `/liella`, so counts follow the tree, not the host or role. NOT VERIFIED: the relay
  binaries on Skylake-SP / Zen 1 (the CI gate on Skylake-SP will show them).
* One bound per mode at max(fast, release) + 1, not per-profile: the gate builds fast only (reason in D-X.53 (4)).
* Merge with next raised fast locale/tool/fork by 1 each before the relay (56/59/6 -> 57/60/7, layout-only); next's
  own counts were not measured (budget). Release vs next not compared directly; all counts sit far under next's
  bounds (23 / 68 / 71 / 9).

## Open / next
* If the CI gate on Skylake-SP reads differently from the table, take its counts (both passes print the same now).
* Structural fix for `.data`/`.bss` start-up faults: generate the `.data1` list from a trace of first-touched statics
  (as regen.sh does for text) instead of the hand-kept `STATE_KEY_SUFFIXES` (a `.data` reorder alone moves NSCACHE).
* regen.sh `--build`: run prune.py once both profiles' `hot-text.matched` come from the same tree (needs a tree-hash
  guard: a stale other-profile file would drop live keys).

## Jobs (run 4, shiki unless named)
Diagnosis: -measure1 (step-0 clippy + static green on the inherited head; fast/release counts, replica = test),
mei:-replica-mei, keke:-replica-keke (host classes), -interposed1 (reproduced CI's red), -relay1 / -relay2 (killed or
superseded: dead ends above), -relay3 (final harness, all 4 configurations green). Final head: -gate1 (clippy, static,
records check, affected --base origin/next) and -plants (2 plants).
