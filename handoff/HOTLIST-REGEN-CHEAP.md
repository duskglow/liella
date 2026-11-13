# HOTLIST-REGEN-CHEAP: one hot-list regen of both profiles in 5.4 min on mei, byte-identical, deterministic (INCOMPLETE: no plants yet)

State: branch `task/hotlist-regen-cheap`, from origin/next 9b2a0c029. Code commits ad55853f4 (cheap path) and
the so.rs fix after it, then the pending record `records/pending/HOTLIST-REGEN-CHEAP-1.toml` (D-NEW, series D-X).
clippy (workspace, all targets, -D warnings, plus `cargo xtask clippy`) and `cargo xtask static` pass on the
final head (jobs hrc-clippy2, hrc-static3 on mei). Not done: the parallel-runner test and the two plants
(see "Next step"). Gate: `cargo xtask affected --list --base origin/next` escalates to FULL (xtask/src/main.rs,
so.rs, hottext.rs are product-path modules), so no developer gate was run. The train's FULL gate covers it.

## What changed
- `cargo xtask so --hottext-regen --libc-only <dir>` (so.rs `run_regen_libc`): only `libc.a` and `liella-crt1.o`
  (built as `so` builds them: `build_archive_locked`, then the shared `build_crt1`) plus the `.profile` marker,
  into `<dir>`. It never writes `target/liella`. A regen build also saves the archive as the grouping receives it
  (`target/product/<p>/hot-text.pre-group.a`).
- `cargo xtask hottext-recheck`: reruns `hottext::group` (grouping, cluster check, dead-key check, all enforced)
  on a copy of each profile's saved archive with the rewritten list. It replaces the second full `so`.
- `tools/hottext/regen.sh`: `--products <dir>`, `--trace-only <new dir>` / `--merge-only <dir>` (phase split), and
  parallel traces (`HOTTEXT_JOBS`, default nproc). Each trace keeps its number, so the merge order stays fixed.
  The `p_*` call checks now run after all traces. The probe-text modes trace in the same phase
  (`probe_text.sh --trace-only/--merge-only/--products`). `startup_data.sh` gets the products through
  `STARTUP_DATA_PRODUCTS`.
  `--both` is the cheap regen of both profiles. It builds fast, starts the fast traces, builds release, starts
  the release traces, then merges fast. If that merge changed any file the release build reads (hot-text.list,
  startup-text.keys, probe-text.keys, startup-data.keys), it rebuilds and retraces release. Then it merges
  release and runs the recheck. It prints `regen-both: phase <name>: <s>` lines.
  `HOTTEXT_GDB` overrides the tracer command. It was added for a test that is not written yet.

## Measured on mei (48 threads; job hrc-regen-cmp, script ~/work/HOTLIST-REGEN-CHEAP/cmp.sh, same tree, both profiles, merge mode, --allow-drop)
| path | fast | release | total |
|---|---|---|---|
| full (origin/next's scripts, `regen.sh . --build` per profile) | 483 s | 586 s | **1069 s (17.8 min)** |
| cheap (`regen.sh . --both`) | build 77 s | build 143 s (fast traces ran during it) | **324 s (5.4 min)** |

Cheap path, by phase: build-fast 77, build-release 143, traces-both 7, merge-fast 5, rebuild-retrace-release 88,
merge-release 3, recheck 1 (seconds). Load was 0.9 at the start and 9 before the cheap run.
**Outputs: hot-text.list, startup-text.keys, probe-text.keys, startup-data.keys and hot-text.absent are all
IDENTICAL (cmp) between the two paths.** Both paths changed probe-text.keys (1 line) and startup-data.keys
(33 lines) relative to the tracked copies.
Open question: on this tree the fast merge changed all four inputs of the release build, yet the final
hot-text.list and startup-text.keys equal the tracked ones. So the release products were rebuilt (88 s), and the
speculative release build and traces were wasted. Why a fast merge of the tracked lists is not a no-op for
each profile on its own (merge.py's profile columns?) is not yet understood. If it is the steady state, the
speculative release build could be dropped to save CPU (not time).

## Determinism (#5084): settled, deterministic
One product set (the cheap path's fast and release products, built from the tracked lists) and the tracked
startup-text/startup-data keys were bundled. The data trace (`startup_data.sh`, both profiles) ran twice on keke
(job hrc-det-keke, load about 10.7) and once on mei (hrc-det-mei, load about 4.5). The sha256 values are the same
in all three runs (fast c7e75a509774c134, release e2fc01680c6b6a5a). The trace also reproduces on the full path's
check build (products built from the rewritten lists): "release data trace on the check build: same output".
So the difference from the tracked startup-data.keys does not come from the run or the host. It comes from the
input: the tracked copy is older than this tree's products (33 lines on 9b2a0c029). One regen reaches a fixed
point. Nothing was sorted or filtered.

## Next step (a continuation, about 25 calls)
1. Write the parallel-runner test. A fake tracer (`HOTTEXT_GDB`) prints the program's `nm` T/W names and waits,
   with an iteration budget, until all N traces of one `--trace-only` run have started. A serial trace never
   meets the others, so the run fails. Then compare a `HOTTEXT_JOBS=1` run with a parallel run byte for byte
   (tagged with the directory normalised, names.*, startup.names, probe/names.*). Bake the barrier into the fake
   script: probe_text.sh traces under `env -i`.
2. Map tools/hottext/ to that test in `affected` (tools/ selects nothing today).
3. Plants: (a) run `stdin= follow= run_traced ... page_faults true` without `&` semantics (serial), caught by the
   rendezvous; (b) drop one names file from the cheap path's collection, caught by the comparison. Run them through
   `cargo xtask plants` with msg: regexes using [0-9]+.
4. Then HOTLIST-TRAIN-REGEN step 2 (the train command) can call `regen.sh . --both`.

Plan rows: D-X.49, D-X.50, D-X.62, D-X.63 (the lists' contracts). Prior measurement: handoff/HOTLIST-TRAIN-REGEN.md on
task/hotlist-train-regen.
Process note: one empty `python3 - <<'X'` heredoc was run by mistake (it did nothing). No other rule exception.
Nothing is left running. The cmp job restored the role's lists, and the role was re-synced afterwards.
