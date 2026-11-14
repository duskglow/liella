# LAYOUT-HASWELL-TOOL-51: next reads 51 on the Haswell-EP host against the fast coreutils-shaped bound 50

State: branch `task/layout-haswell-tool-51`, cut from origin/next 630f3afb2. No product code changed: the
51 did not reproduce, so there was no extra fault to name, no landing to bisect and nothing to fix (brief
steps 1-2). This branch carries this note and `handoff/reports/LAYOUT-HASWELL-TOOL-51.json` only. Bounds
unchanged (`TOOL_BOUND` 50 in crates/core/tests/hot_text_products.rs, set by LAYOUT-TRACE-DEFAULT, D-X.92).

## Method (scratch scripts, not in the tree: shiki `~/work/LAYOUT-HASWELL-TOOL-51/`)

* `measure.sh <tree> <slot> <out>`: default `cargo xtask so` (fast products, trace-at-build), then
  `page_faults.c` linked exactly as `hot_text_products.rs` `build` links it.
* `runner.py`: the test's `faults` conditions: a copy at a fixed-length `/tmp/liella-hot-text-<10 digits>-0/p`,
  ADDR_NO_RANDOMIZE, argv0 `page_faults`, cwd `/`, environment `LANG=en_US.UTF-8` only, `relay <mode>`,
  warm-up + 5 runs, lowest kept. Then two runs under `perf record -e page-faults -c 1 -d` (same environment;
  argv0 is the path there, so `true` reads 17 under perf against 11 counted, and `tool` 49 against 50).
* `sym.py`: each fault of the relayed (counted) process as page, section, symbol at the address and at the ip.
* Trees: `git archive <rev>` unpacked whole into `/mnt/zram/claude/LAYOUT-HASWELL-TOOL-51/<name>` on shiki
  (Xeon E5-2697 v3, Haswell-EP, kernel 6.8.0-147).

## Results (shiki, Haswell-EP, fast products; every run identical)

| tree | rev | how | true | locale | tool | perf tool set |
|---|---|---|---|---|---|---|
| next (this branch's base) | 630f3afb2 | `so` + runner | 11 | 46 | **50** | T 8, R 1, R1 1, got 1, D1 2, B 3, O 33 |
| bound-setting landing (LAYOUT-TRACE-DEFAULT-4) | ba8b29cde | `so` + runner | 11 | 46 | **50** | the same pages |
| next | 630f3afb2 | fresh tree, `~/bin/gate 16G`, `affected --only hot_text_products --run` | pass | pass | **pass (<= 50)** | - |
| next after smoke-errno-fixes-3 | 8f3e2993e | `so` + runner | 11 | 46 | **50** | - |

The two perf fault sets (630f3afb2, ba8b29cde) list the same pages per section; the only label differences are
symbol names on the same page (an `.llvm.<hash>` suffix of `TLS_IMAGE`, and one `.bss` page that perf
attributed to `liella_machine_strcmp_rescans` in one build and to no symbol in the other: page 0x876, both
builds). libc.a md5: 630f3afb2 93cfc007..., ba8b29cde f136defc..., 8f3e2993e aa0110a1....

TB-FOREIGN-WORDS-LOW-8 read 51 in its gate in role shiki `~/w/sh1` (SSD, inside `~/bin/gate`, the full direct tier,
which runs `hot_text_products` in two passes). This story reproduced neither in plain builds nor in a gated
`hot_text_products` run of a fresh next tree. The cause of that 51 is not known. It may be a property of that
role's tree or build state, which this story could not reach (it is not this story's role). It may also be host
state at that time: fault-around maps only pages already in the page cache, so a locale-archive page evicted under
load would add an `O` fault without any layout change. Neither is measured; both are hypotheses.

## Proof jobs on this branch's head (brief step 3; started after the last commit)

NOT RUN: the permission layer REFUSED the single call that would start them, and per the rules they were not
retried. The planned jobs were: `proof.sh` (fresh `cargo xtask so`, then `cargo xtask affected --only
hot_text_products --run`, then the runner counts and perf sets) on `git archive` copies of head 40d4de569
(shiki and mei `/mnt/zram/claude/LAYOUT-HASWELL-TOOL-51/head`, keke `/dev/shm/claude/LAYOUT-HASWELL-TOOL-51/head`,
staged there; the scripts are in `~/work/LAYOUT-HASWELL-TOOL-51/` on all four hosts), kanon role kn1 under
`~/bin/gate`, and `layout-haswell-tool-51-lint` (clippy --all-features -D warnings + `cargo xtask static`) on kn1.
So the four-host proof and the lint of this head are NOT VERIFIED. The head adds only this note and the report json
to origin/next 630f3afb2. The only evidence on that base is shiki `-g-next` (gate, rc 0) and `-m-next`.

Diagnostic jobs (evidence, not on the head): shiki `layout-haswell-tool-51-m-next`, `-m-base`, `-m-n2`
(count, rc 0), `-g-next` (gate, rc 0).

## Open / next step

* RULING NEEDED: close the story as not reproduced. If a gate reads 51 again, capture that job's tree (sync
  `.synced` head, libc.a md5) and its perf fault set right away (`runner.py` + `sym.py`, a few seconds on the
  same products), so the extra page is named while the state that produced it still exists.
* Do not redo: bisecting the 20 landings ba8b29cde..630f3afb2. Both ends read 50 on Haswell-EP, so there is
  nothing to bisect.
* Plan rows: D-X.92 (trace-at-build default, zero-margin bounds); handoff/LAYOUT-TRACE-DEFAULT.md.
