# LAYOUT-PLACEMENT-SWEEP: operand placement as a contract (bench offsets, kernel cost over page offsets)

State: branch `task/layout-placement-sweep-2` (continues `task/layout-placement-sweep`; merges `task/layout-manifest-2`,
which carries origin/next 03c9a4972 and the RECORDS cutover). Last run: LAYOUT-PLACEMENT-SWEEP-2, role keke:~/w/n1 (Zen 1).
Decision in force: D-X.52, now the record `records/decisions/D-X/D-X.52.toml` (no plan/07-decisions view is changed by
this branch). Plan: handoff/layout-contracts-plan.md L15, L16.

## What was done
- L15: `bench/c/cbench_hot.c`: the operand block is `struct hot_bufs`; `_Static_assert`s on `__alignof__(bufs) == 4096`
  and, per buffer, `offsetof % 4096 != 0 && <= 4032`. A reorder or a new buffer at a bad offset fails the bench build.
- L16: `crates/boundary/tests/placement_sweep.rs` (no spec clause): a static program built with liella-cc counts user
  instructions per call (perf_event_open, 64 calls per placement) for strcasecmp/strncasecmp (L 256 / n 64),
  strcmp/strncmp (256 / 64), memcmp 256, strlen 256, strnlen 256 (max 320), memchr 256 (miss), rawmemchr 256, with
  each operand at page offsets {0,1,15,16,31,32,4032,4064,4065,4080,4095} and every pair. Two bounds per kernel,
  worst/median: all placements (K_all) and head placements, every offset <= 32 (K_head). SKIP (printed) without AVX2 or
  when perf_event_open is refused. K per kernel = measured spread + about 10% (one K would be ~530% and blind to casecmp).
- Run 2: merged task/layout-manifest-2 (conflict only in the generated view x-implementation-decisions.md: took that
  branch's copy); D-X.52 moved from the view row to its record (`records check` ok).

## Calibration: Skylake-SP (run 1) and Zen 1 (run 2, keke job lps2-zen1) give IDENTICAL counts
worst/median %, all / head: strcasecmp 280/104, strncasecmp 527/113, strcmp 426/130, strncmp 470/128, memcmp 100/100,
strlen 125/100, strnlen 126/100, memchr 102/100, rawmemchr 104/100 (medians 302, 133, 211, 123, 71, 87, 110, 103, 93).
Same AVX2 kernels on both classes, so the bounds hold unchanged on Zen 1 (resolves run 1's open item, #4149).

## Results (run 2, keke, tree ce61aa988; test and bench code unchanged since run 1; later commits are handoff only)
- Lint: lps2-lint0 (inherited head) and lps2-lint1 (records check, workspace clippy --all-features -D warnings, static): rc 0.
- Gate lps2-gate: `cargo xtask affected --run --files bench/c/cbench_hot.c crates/boundary/tests/placement_sweep.rs`
  (this story's own files; direct tier): 4 suites, 311 passed, 0 failed, SKIPPED lines 0, peak 1.3G, rc 0.
  Why `--files`: `--base origin/next` escalates to FULL on the stacked LAYOUT-MANIFEST files (layout/contracts.tsv,
  xtask/src/{classed,jcc,layout}.rs) and on records/decisions/D-X/*.toml ("not a path the selection knows"), #4147.
- Plants lps2-plants (`handoff/LAYOUT-PLACEMENT-SWEEP-plants.txt`, results appended): 2 CAUGHT, 0 SURVIVED, no
  WARN-RC-ONLY / WARN-NO-MSG (WARN-DEV-SHM only: the copy was under /dev/shm; these tests mount nothing).
  lps-casecmp-head-generic: "strncasecmp: N instructions at head placement"; lps-bench-reorder: "a64 at page offset 0".

## Open / next step
- affected does not classify `records/` paths: a decision record alone escalates a branch to FULL (informational).
- Placement spreads worth a story: strcmp 900 at 32/4065 (426%), strncmp 579 (470%), strncasecmp 701 (527%),
  strcasecmp 848 (280%); strcmp's head spread 130% (an operand at page offset 0). Each fix tightens its K.
- Manifest: a `build-assert` kind would let L15 be a manifest row (BENCH-OPERAND-OFFSETS) now that the manifest exists.
- Next step: none for this story; land it. Follow-ups above are separate stories.
