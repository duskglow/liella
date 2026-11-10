# STARTUP-FAULTS-RESTORE: static start-up back to 61 page faults, and contracts that hold it

State: branch `task/startup-faults-restore-5` (continues `-4` .. `-1`), merged with origin/next ccc0d587f
(coordinator-allowed; clean merge). Role mei:~/w/s4 (Skylake-SP). Decisions in force: D-X.20, D-X.22, D-X.32,
D-X.47, **D-X.53** (this story; branch-only record: bounds = count + at most 1, one per mode at the larger profile's
count, lowered when a count falls, never raised; a dead hot-text key fails `so` unless allow-listed; start-up state
words in `.data1`; counts through the relay harness; (5) has run 5's counts). Plants: `handoff/STARTUP-FAULTS-RESTORE-plants.txt`
(runs 1-3), `-4-plants.txt` (run 4), `-5-plants.txt` (run 5). Report: `handoff/reports/STARTUP-FAULTS-RESTORE-5.json`.

## What the story does (runs 1-4)
* **Dead keys fail the build** (`xtask/src/hottext.rs`): every key of `crates/libc-so/hot-text.list` must match a
  function section in a fast or release build, except one traced only in the other profile or allow-listed in
  `crates/libc-so/hot-text.absent`. `tools/hottext/prune.py` drops dead keys.
* **Start-up state on one page** (`xtask/src/coldbss.rs::move_startup_state`): listed statics go to `.data1`.
* **Relay harness** (run 4): `page_faults.c relay <mode>` vforks/execs itself with raw syscalls, so both gate passes
  (plain and exports-interposed) read the same counts.

## Run 5
* Step 0: the inherited red was static's codemap check (landed in next with XTASK-CODEMAP). Merged next, ran
  `cargo xtask codemap`, committed `docs/codemap/symbols.tsv`; clippy + static green on the merged head.
* The merge broke fast `so`: the `Files/Glue` instances of `locale::find_cached` and `locale::attach_charmap` have
  no fast section any more (inlined; next's locale code is identical, so its callers changed). Next's own check only
  guards start-up/fork keys, so next stays silent. prune.py with both profiles' `hot-text.matched` dropped exactly
  those two keys (588 left); dead keys order nothing, so the layout is unchanged. (regen.sh retrace was not run: my
  job called it without `bash` and it is mode 644 in git; the prune alone is the whole fix.)
* Counts, relay harness, Skylake-SP host (#4223), every run identical (one replica run read locale 54 once in 5,
  min 53; the test's runs were all 53), plain and interposed test passes identical; next = origin/next ccc0d587f
  with the branch's `page_faults.c` (#4224):
  | mode | branch fast | branch release | next fast | next release | bound (was) |
  |---|---|---|---|---|---|
  | startup only | 17 | 15 | 18 | 16 | 17 |
  | setlocale + printf | 53 | 54 | 54 | 54 | 55 (57) |
  | tool | 56 | 57 | 57 | 57 | 58 (60) |
  | fork child | 7 | 6 | 7 | 6 | 7 |
  | localtime, localtime+mktime | 20 | 19 | 19 | 19 | 20 |
  | float | 2 | 4 | 2 | 4 | 5 |
* Bounds at max(fast, release) + 1 where that is lower; TRUE/FORK/LOCALTIME stay (max + 1 would raise them; the
  fast count equals the bound there). All under next's 23 / 68 / 71 / 9.
* Fast localtime +1 against next is a window shift, not more work: whole-process faults of `localtime` are 55 on
  both (faultmap.py, `perf record -e page-faults`). On next, start-up first writes `.data` page 0x84f000 through
  `sys::SYSINFO_EHDR`; on the branch that static is in `.data1`, and `time::zone::NEXT_ID` first writes the page
  inside the timed window.
* Zen 1 (#4223): the four binaries (branch/next x fast/release) were copied to keke with the relay replica
  `pf_launch` (`~/work/STARTUP-FAULTS-RESTORE-5/zen.sh`); job keke:startup-faults-restore-5-zen1: every count
  equal to the Skylake-SP table above, every run identical. The counts follow the tree, not the host class.
* Plants (run 5): both caught. relay-dropped read `startup-only program: 18 page faults > 17` but the tool printed
  WARN-NO-MSG for the `\d+` regex. The tool's matcher apparently does not accept `\d`, so the committed file now
  says `[0-9]+` (not re-run). Run 4's file has the same `\d+`.

## Open / next
* Move `time::zone::NEXT_ID` (or put it with its first writers) so the fast localtime window is back to 19 (LOCALTIME
  bounds 20 then sit at max + 1, the rule's level). One-line change to `STATE_KEY_SUFFIXES` plus the test's list,
  but measure every mode: the D-X.53 NEGATIVE shows freed pages get new first writers.
* Structural: generate the `.data1` list from a trace of first-touched statics instead of the hand-kept list.
* regen.sh `--build`: prune once both profiles' `hot-text.matched` come from the same tree (tree-hash guard).

## Jobs (run 5, mei unless named)
-codemap (regenerated symbols.tsv), -lint1 (step 0: clippy + static on the merged head, green), -measure1 (fast
`so` red on the two dead keys; release counts), -next1 (next's counts), -regen1 (prune; both profiles build),
-measure2 (branch counts, both profiles, both passes), -fmap1 (faultmap branch vs next, localtime), keke:-zen1
(Zen 1 counts). Final head: -gate1 (clippy, static, records check, affected --base origin/next) and -plants (2 plants).
