# TVA-XTASK-RUN-ONCE: dependency-free test binaries run once in the exports-interposed pass

State: branch `task/tva-xtask-run-once` from `origin/next` (a09927ac8); build role keke:~/w/n4.
Decisions: none new (no D-row; local gate-tooling detail, as CI-GATE-DIET before it).

## The change (xtask/src/nextest.rs)

- `unchanged_binaries` reads `cargo metadata --format-version 1 --offline <features>` once and builds the
  `resolve` graph (`PackageGraph`, every dependency kind: a test binary links its package's dev-dependencies).
- `run_once_reason`: a binary runs once when its package (nextest's `package-id`) has no path to
  liella-boundary in that graph (`no dependency on liella-boundary`), else when its bytes lack the marker
  (CI-GATE-DIET's byte rule, unchanged: `no \`liella_boundary\` in the binary`). Each `once:` line prints its reason.
- Fails safe: no metadata, no resolve graph, no liella-boundary package, an empty package id or a package
  outside the graph = not proven independent; the byte rule alone decides.
- The byte marker itself is not narrowed.

## Which packages the graph frees

Workspace packages and their liella-boundary dependency (Cargo.toml, resolve graph on keke):
depends: liella (core), liella-rt, liella-math, liella-boundary (itself), and everything above them.
Freed by the graph (pass-2 listing on keke): xtask, liella-kernel, liella-mem, liella-mathtools, liella-thread-db,
spectest (the last two list no tests). liella-abi, liella-ld and liella-legacy reach
liella-boundary in the graph and keep the byte rule (their binaries were already run-once by it).
A freed binary cannot link liella-boundary, so the export features cannot change its code; a freed test
that runs a C program against the products (`target/liella/`) sees the same products in both passes, since the
export features change only the test process, not the products (reasoning from xtask/src/main.rs: `so::run_so`
builds the products once before both passes; not separately verified by a run).

## Proof

- Unit tests: `a_package_reaching_boundary_through_another_is_not_independent` (A -> B -> liella-boundary is not
  independent; a package outside the graph is not), `without_a_package_graph_only_the_bytes_decide`, existing
  `a_binary_holding_boundary_code_is_never_run_once` (listed_binaries now also returns the package id).
- Plants (`cargo xtask plants`, file in the report): graph walks direct deps only; no graph = all once; byte
  rule dropped in `may_hold_boundary_code`; byte rule skipped at the call site.
- Pass-2 listing: the real `xtask test` run with a listing-only cargo wrapper (scratch, not in the tree):
  `nextest run` -> `nextest list`, stopping at the exports pass's doctests.

## Next step

None for this story. Not redone: the byte marker (kept), CI-GATE-DIET's `PASS0_ONLY` list.

Report: `handoff/reports/TVA-XTASK-RUN-ONCE.json`; results below.

## Results

- Unit tests (keke, job `tva-run-once-unit`): `cargo test -p xtask --bin xtask nextest::` 9 passed.
- Plants (`tva-xtask-run-once-plants`, keke, file `~/work/TVA-XTASK-RUN-ONCE/plants.txt`): 4 plants, 4 CAUGHT,
  0 SURVIVED, 0 WARN-RC-ONLY, 0 WARN-NO-MSG (graph-direct-only, nograph-all-once, byte-rule-dropped, byte-rule-skipped).
- Pass-2 listing (`tva-run-once-listing`, real `xtask test`, listing-only cargo): `once: xtask::bin/xtask (no
  dependency on liella-boundary)` and `once: xtask::affected_mode (...)`; 85 binaries run once. Pass 1 lists 6111
  tests, pass 2 4762. The graph-freed binaries hold 292 pass-1 tests (xtask::bin/xtask 238, xtask::affected_mode 3,
  liella-mathtools 36, liella-kernel 10, liella-mem 5); none appear in pass 2. The 241 xtask tests held the marker
  (`a_binary_holding_boundary_code_is_never_run_once`), so pass 2 is at least -241; whether the other 51 were
  already freed by the byte rule is NOT VERIFIED (no before-listing was taken). Time saved per train: NOT VERIFIED.
- Gate (`tva-xtask-run-once-gate`, direct tier `--base origin/next`, with `xtask clippy` and `xtask static` first):
  started after the listing job; `affected` escalated it to FULL (xtask change), clippy ok, `xtask test` still running at hand-back.
- Leftover: none running of mine apart from the gate job named above.
