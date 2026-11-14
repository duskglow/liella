# DL-DLSYM-3 report (continuation of DL-DLSYM-2, phase 14 DL-16b)

Branch `task/dl-dlsym-3`; role shiki:~/w/sh1; measurements of ruling #5350 on mei (RAM trees under
/mnt/zram/claude/dl-dlsym-3/, scripts in mei ~/work/DL-DLSYM-3/). Summary and next step: handoff/DL-DLSYM.md.

## Commits (this run)
- `183cb1d89` merge of `origin/next` d483eefbe (the base DL-DLOPEN-LOAD-4 landed; rulings #5331, #5350). Conflicts:
  git guessed that `records/pending/DL-DLSYM-2-{1,2}.toml` moved to `records/compat/` (next renamed the
  DL-DLOPEN-LOAD-3 pending records into assigned ids there); both stay in `records/pending/`, unchanged.
  `docs/differences-from-glibc.md`: next's rows (assigned COMPAT ids) plus this branch's object-capacity row
  (kept by ruling #5351). `s27_1_f_0`: next's stronger check (run.sh summary `1 passed, 0 failed`).
- `b74793205` handoff; the report and json commit on top. No product code changed in this run.

## Step 0
`dl-dlsym-3-lint0` (inherited head 53f881849): `cargo xtask clippy` and `cargo xtask static`, rc 0.

## Ruling #5350: where the start-up layout red comes from
Test: `hot_text_products::init_data_clusters_hold_at_every_text_phase` (static probe `page_faults.c` linked against
`libc.a` with a padded start file; the `true`, `tool` and `fork` counts must be equal at every `cold`, `lbss` and
`data` pad). Mei (Skylake-SP, the CI host class), fresh `git archive` trees, `cargo xtask so` then the test alone,
fast profile then release (`LIELLA_FULL_PROFILE=1`):

| tree | fast: tool at cold 0x800 / 0x2000 / 0x8000 / 0xf000, lbss, data | release: tool, every pad |
|---|---|---|
| next d483eefbe (`dl-dlsym-3-hot-next`, rc 0) | 53 / 53 / 53 / 53, 53, 53: pass | 55: pass |
| this head 183cb1d89 (`dl-dlsym-3-hot-head`, rc 1) | 53 / **54** / 53 / 53, 53, 53: FAIL | 55: pass |

`true` (11 fast, 10 release) and `fork` (5) are equal everywhere in both trees. So only this branch moves it, and
only in the fast profile.

Which page (`dl-dlsym-3-diag-head`, `-diag-next`: the probe relinked as the test links it,
`setarch -R perf record -e page-faults -c 1 -d` of `tool`, classified with tools/hottext/faultmap.py): at
`cold 0x2000` this head has 11 `.text` faults against 10; the extra one is the second page of
`liella_legacy::global_locale::setlocale_locked::<CSysImpl, Glue>` (0x64fa40 .. past 0x650000). That function
lies far outside the hot group in both trees (next: 0x685000, one fault; this head: 0x64e000/0x64f000), because
next's tracked `crates/libc-so/hot-text.list` lists the generic `global_locale::setlocale` but not
`setlocale_locked`, which SETLOCALE-C-ROWS keeps out of line. Its offset in cold text moves with any change to
the library's code size or codegen-unit partition; this branch's library code (`liella_dl_sym`, the dlsym entries,
`LOADER_DLSYM`) moves it so that one pad splits it over two pages. `StartInfo::dlsym` is not the cause: a static
program never calls `runtime::early_init` with a start description, and the probe calls no `dl*` function.


Is regenerating the lists a way out? No, not today. `cargo xtask hot-lists` (branch mode: target/hot-lists with a
stamp of the tree) adds `setlocale_locked` and gives tool **51 at every pad** (the test passes), but the regenerated
`hot-text.list` (60 keys added, 25 dropped against the tracked one) then fails two other layout tests,
`init_zero_fault_clusters_sit_in_one_window` and `init_zero_fault_calls_hold_at_every_text_phase` ("cluster
strings at 0x419640..0x485ea5 straddles a 64 KiB window", likewise time, rebind-tail, float): the zero-fault
clusters of `hot-text.clusters` no longer line up with the regenerated order. That happens on origin/next alone,
with identical addresses (`dl-dlsym-3-nextregen`, mei, rc 1), and on this head (`dl-dlsym-3-regen` shiki,
`dl-dlsym-3-gate` shiki, rc 1). So the stale tracked list cannot be refreshed by a branch today, and the train
evidently does not regenerate it before its gate either (next lands green with it). Nothing in this branch can
fix that without editing a train-owned list or moving code for layout's sake (ruled out by #4347 and by the
generated-file rule); the copy the regeneration wrote in the role was removed before the plain gate.

## Gate and plants (final code; head b74793205's tree, handoff-only commits after it)
- `dl-dlsym-3-lint1`: `cargo xtask clippy`, `cargo xtask static`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`: rc 0.
- `dl-dlsym-3-plants`: `cargo xtask plants handoff/DL-DLSYM-2-plants.txt`: 12 plants, 12 CAUGHT, 0 SURVIVED,
  0 EQUIVALENT, 0 WARN-RC-ONLY, 0 WARN-NO-MSG (rc 0).
- `dl-dlsym-3-gate2`: plain `cargo xtask affected --run --base origin/next` (escalates itself to FULL for
  `xtask/src/compat.rs` and `csys/src/machine/dl.S`, as in run 2), tracked lists: rc 1, 6524 passed, 1 failed, 663 ignored, peak 11G; the one failure is
  `init_data_clusters_hold_at_every_text_phase` (tool 54 at `cold 0x2000`, 53 at every other pad), the layout red
  above. Everything else is green.
- Superseded: `dl-dlsym-3-gate` (same gate after `cargo xtask hot-lists`: 6523 passed, 2 failed, the two cluster
  tests above; peak 13G; the regeneration path, not the branch).

## RULING NEEDED
- Ruling #5350's outcome: only this branch moves the `tool` count (53 -> 54 at `cold 0x2000`, fast profile only),
  but the cause is next's stale tracked hot-text list (`setlocale_locked`, on the tool path, lies in cold text),
  not this branch's start-up code (`StartInfo::dlsym` is never read by a static program; the probe calls no `dl*`).
  It is the class trace-at-build removes. Recommend: hold as LAYOUT-JITTER-CLASS until trace-at-build lands (or the
  train-owned lists are refreshed with consistent clusters), then land unchanged.

## INFORMATIONAL
- next's tracked `hot-text.list` is behind its own trace: a regeneration adds 60 keys and drops 25
  (`setlocale_locked`, nscache, nss, passwd, `builtin_charset`, `CtypeFile::wide`, ...); the `tool` probe pays
  2 faults for it (51 regenerated vs 53 tracked, every pad). And the regenerated list breaks the zero-fault cluster
  windows on next itself, so `cargo xtask hot-lists` is not usable to refresh it as it stands. For the
  HOTLIST-TRAIN-REGEN / trace-at-build owners.
- Diagnostics (evidence, not jobs of the final head): mei `dl-dlsym-3-hot-next` (rc 0), `dl-dlsym-3-hot-head`
  (rc 1, reproduces the red), `dl-dlsym-3-diag-head`, `dl-dlsym-3-diag-next`, `dl-dlsym-3-nextregen` (rc 1);
  shiki `dl-dlsym-3-regen` (rc 0).
