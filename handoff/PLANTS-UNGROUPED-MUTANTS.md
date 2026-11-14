# PLANTS-UNGROUPED-MUTANTS: plants build mutants ungrouped unless the plant targets the layout

## State

- Branch `task/plants-ungrouped-mutants` (from `origin/next` 955b2a375). Code commit 35466247c; HEAD: see the report json.
- Decision: pending record `records/pending/PLANTS-UNGROUPED-MUTANTS-1.toml` (series D-X; the coordinator assigns the id).

## What changed

- `xtask/src/plants.rs`: every product build and every plant command runs with `LIELLA_LAYOUT_TRACE` set by the tool:
  `ungrouped` for an ordinary plant (its rebuild is a compile only), `1` (traced) for a plant that measures the
  start-up layout. The tool overrides the caller's value.
- Marking rule (`targets_layout`, documented in `docs/plants.md`): traced when the command names, as a word, one of
  `cargo xtask affected`'s `LAYOUT_TESTS` (`hot_text_products`, `heap_meta_products`, `cold_bss_products`) or
  `layout-check`; or is `cargo xtask affected ... --run` without `--only` (it selects the layout tests for any change
  that reaches `libc.a`); or the plant ends with the new optional field `layout:traced` (any other `layout:` value
  is refused). A traced plant always rebuilds the products.
- Baseline: one product build per layout the file uses (`so.log` ungrouped first, then `so-traced.log` if the file
  has both), each distinct command run in its plants' layout. A baseline command that fails ungrouped prints a hint
  to mark `layout:traced`.
- The `so` lines and every verdict line name the layout (`build: the products, rebuilt with the mutant, layout
  ungrouped, in <n>s`; `the baseline products (layout <x>), not rebuilt`).
- The unmutated restore build (before a plant that does not read the products) is ungrouped.
- `xtask/src/affected.rs`: `LAYOUT_TESTS` is `pub(crate)` so the plants tool uses the same list.

## Proof

- Unit tests (toy tree, fake prepare commands recording `$LIELLA_LAYOUT_TRACE`):
  `ordinary_plants_build_ungrouped_and_layout_plants_traced` (an ordinary, a `layout:traced` and a
  `hot_text_products`-named plant: builds `ungrouped, 1, ungrouped, 1, 1`, commands see their own layout),
  `a_file_without_layout_plants_builds_ungrouped_only`, `which_commands_target_the_layout`, `plants_lines_parse`.
- Plants on this change: 8 of 8 CAUGHT with msg:, no WARN (`handoff/PLANTS-UNGROUPED-MUTANTS-plants.txt`).
- Gate (affected --run --base origin/next, keke): 2 suites, 394 passed, 0 failed, peak 1.1G.
- clippy and static clean on keke.
- DL-DLADDR-2's plants file rerun on keke (its branch tree, from `git archive origin/task/dl-dladdr-2`, run with
  this branch's xtask binary from `~/w/n2`): see below.

## DL-DLADDR-2 rerun (keke)

| Build | layout | time |
|---|---|---|
| baseline products, cold (pum-dladdr2) | ungrouped | 280 s |
| baseline products, incremental after an ungrouped build (pum-dladdr, `so-traced`) | traced | 779 s (before) |
| each of the 9 mutants' products (pum-dladdr2) | ungrouped | 169-174 s (after) |

- pum-dladdr2 (the unmodified file): 9 of 9 CAUGHT, no WARN, whole job 2459 s (41 min). On kanon the same file
  ran past the 150-min cap after 8 plants (brief; about 520 s per traced rebuild there, not measured here).
- pum-dladdr (rc 2, superseded): the same file with its first plant marked `layout:traced` to time a traced mutant
  on the same host. Its traced baseline command, `cargo xtask affected ... --run`, runs `cargo xtask so` itself,
  which traces AGAIN (> 600 s on keke) and hit the 600 s per-command limit before any plant. So a traced plant
  whose command builds the products pays the trace twice; ordinary plants do not (their command's build is
  ungrouped too). The traced rebuild time above comes from that run's `so-traced` step.

## Open questions / not done

- Follow-up worth a story: when a plant's command builds the products itself (`cargo xtask affected --run`), the
  tool's own rebuild is a second build per mutant. Skipping it (or running such a command under the prepare
  timeout) would halve ordinary plants' time again and keep traced ones under the 600 s limit.

- `cargo test -p liella` with no `--test` (all targets, layout tests included) is not detected; its baseline fails
  ungrouped with the hint, and `layout:traced` fixes it. Detection could reuse `cmd_uses_products`' parser if wanted.
- Rule slip in this run: two shell heredocs (a Python edit of `plants.rs` and the first commit message) where the
  brief forbids heredocs; the results are ordinary file contents, reviewed in the diff.

## Next step

Land. After landing, drop the brief-level interim rule ("run plants jobs with LIELLA_LAYOUT_TRACE=ungrouped"): the
tool now sets it per plant.
