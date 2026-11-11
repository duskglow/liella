# LAYOUT-FAULT-CONTRACTS (brief C of LAYOUT-STABILITY-RETHINK): regen items done, fault contracts not started

**State (2026-11-11):** branch `task/layout-fault-contracts` = task/layout-text-pin-9 81c825fdf (stacks on
LAYOUT-TEXT-PIN, not landed) + a merge of `origin/task/hottext-regen-hosts-2` 6a56f31dc (96cc93374; the brief:
"merge it, do not duplicate it") + this run's commits. Not merged with origin/next (25 commits ahead of the base; the
brief allows no other merge). Role mei:~/w/s3. Decisions in force: D-X.65 (this run), D-X.61 (per-profile lists,
host-class provenance), D-X.63, D-X.56, D-X.49, D-X.47; rulings #4349 (perf contracts), #4539, #4541.
Design: handoff/layout-stability-rethink.md sections 3-4.

## Done
1. **Merge of HOTTEXT-REGEN-HOSTS-2** (brief item 3.1: one list per profile; the file names are that branch's
   `hot-text-fast.list` / `hot-text-release.list`, not the brief's `hot-text.fast.list`). Conflicts resolved by
   keeping both sides: merge.py has host-class provenance AND pin-9's `--startup-head`, `--alias`, `--dropped`
   (dropped lines now name their pairs); regen.sh has both. The lists: pin-9's regenerated shared list (both profiles
   traced on a Haswell-EP host, `intel-6-63`) split by the D-X.61 rule (533 fast, 471 release keys), keeping
   hosts-2's host-class pairs; startup-text.keys the same (111 keys). Readers of the old single file fixed:
   startup_ro.rs (union of both lists, as before the split), the start-up-head unit test (both lists), the
   hot_text_products.rs cluster reader (the products' profile). probe_text.sh called merge.py without the host
   class the merge made mandatory: fixed (regen.sh exports `HOTTEXT_HOST_CLASS`).
2. **Item 3.3:** the regen build reports a broken layout contract and goes on (`layout::regen_verdict`); the plain
   `so` after the regen enforces.
3. **Item 3.4, hermetic trace:** `env -i` (LANG fixed; LC_ALL and other LC_* unset, as the bound tests run; TZ passed
   only where a probe sets it: the merged `env -i` had silently dropped p_cold's America/New_York zone), gdb's
   default disable-randomization, a fixed-length `/tmp/hottext.XXXXXX` work path whatever TMPDIR says, every helper
   as `python3 -B`, usage `bash tools/hottext/regen.sh` (it keeps its exec bit).
4. Tests (no spec clause: build tooling): `hottext::tests::regen_traces_in_a_hermetic_environment` (also: regen.sh
   names only its own profile's list), `layout::tests::a_broken_contract_is_reported_in_the_regen_build_and_fails_any_other`;
   pin-9's three merge tests now pass a host class and expect provenance.
5. **perf on the build hosts (ruling #4349 asked):** `perf record -e page-faults -d` works for `claude` on mei, keke,
   shiki and kanon (perf_event_paranoid 1, kernels 6.8.0-147/149; a `/bin/true` record gave 7-8 addressed samples on
   each). The key-based fallback is not needed on any current build host.

## Jobs (mei)
lfc-lint3 (clippy, static, xtask hottext+layout tests: 37 passed) rc 0; lfc-plants: 6 CAUGHT, 0 SURVIVED, 0 WARN
(handoff/LAYOUT-FAULT-CONTRACTS-plants.txt); lfc-gate (`affected --run --base origin/next`): see the report json.
Superseded: lfc-lint1 (3 merge tests needed the host class), lfc-lint2 (static: layout.rs named the regen flag).

## Not done (next step, in order)
1. **Brief item 1, the perf contracts.** Plan: a `fault-classes` contract kind in `layout/contracts.tsv`: link
   page_faults.c with a map as `link_startup_probe` does; for each mode (true, locale, tool, localtime_mktime, float)
   run `setarch -R perf record -e page-faults -d -o F -- ./page_faults <mode>` (env -i, LANG=en_US.UTF-8, stdin
   /dev/null, cwd /; enable-on-exec counts from exec) and `perf script -F addr`; classify each address by the probe's
   `readelf -SW` / `readelf -lW` (GNU_RELRO range): text in the hot group (map: `.text.startup*`..`.text.sorted.h*`),
   text outside it (bound 0), read-only (cluster `.rodata1`), relro, written (`.data`/`.data1`/`.bss`/`.lbss`,
   cluster `.data1`), O (outside the image, unbounded). Bounds `<class>-<profile>=` from a both-profile measurement on
   one host; a rise prints every fault of that class with `nm -n` symbol + offset. Then STARTUP-TEXT-WINDOWS /
   STARTUP-DATA-WINDOWS become report-only where perf works (key lists = placement input only).
   One shared script (e.g. `tools/layout/fault_classes.py`) can serve layout.rs and the phase test.
2. **Item 2, phase test:** extend `init_zero_fault_calls_hold_at_every_text_phase` (pads 0x800/0x2000/0x8000/0xF000,
   pre and cold, `padded_crt1`) to require identical per-class counts across the 8 links (skip with a printed reason
   when perf is absent).
3. **Regen item 2:** trace every page_faults mode (not only true/locale/tool) so getrusage gets its real tier (#4327).
4. Plants still owed by the brief: a start-up function moved out of the group (named at its fault address).
   Done here: the fast regen rewriting the release list and the regen build failing on a contract.
5. Regenerate both profiles' lists on the merged tree on two host classes (hosts-2's open step 2) once 1-3 land.

## Not to redo
The merge and its list split; the hermetic changes; the perf host survey above.
