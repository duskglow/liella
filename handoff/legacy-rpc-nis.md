# Handoff: legacy RPC/NIS lane (step 2 done: libnsl.so.1 placeholder, LEGACY-RPC-LIBNSL-PLACEHOLDER)

**State:** branch `task/legacy-rpc-libnsl-placeholder` (cut from origin/next 6faf9548), HEAD: see
`handoff/reports/LEGACY-RPC-LIBNSL-PLACEHOLDER.json`. Role **keke:~/w/r5**.
**Decisions in force:** D-29.6 (new: libnsl.so.1 placeholder and its failure table), D-29.5 (RPC client/server stubs),
D-29.4 (secure-RPC/DES stubs), D-27.9 (placeholder machinery), D-15.7 (rcmd family); rulings R13, R14-12 (a), R14-3.

## Done this run (lane step 2)
- `crates/libc-so/placeholders/libnsl.tsv`: the failure table, one row per libnsl.so.1 export (121; the 6 `GLIBC_PRIVATE`
  rows are left out, R14-12): kind (`int`, `null`, `result`, `void`, `free`, `text`, `textbuf`, `perror`) and value.
- `xtask/src/placeholders.rs`: `libnsl.so.1` in `LIBRARIES`; `stub_table`/`stub_bodies` turn a `<lib>.tsv` into the C
  bodies (`target/product/placeholders/libnsl.so.1.bodies.c`, generated at `cargo xtask so`), refusing a table that misses
  an export or names a non-export; unit test `a_failure_table_gives_one_body_per_export`. No `libnsl.a` (all rows compat).
- Values come from black-box probes of the reference on this host class with no NIS domain/ypbind/cold-start file
  (probe sources in keke `~/work/legacy-rpc-libnsl-placeholder/probe*.c`): YPERR codes per call, NIS+ results a 56-byte
  object with status 41 (`NIS_UNAVAIL`), `nis_dir_cmp` enum 0..3. Full table and harmless choices: D-29.6.
- Test `crates/core/tests/s29_legacy_clauses.rs` `s29_2_c_0` (clause specified): table = versions.tsv rows (all compat);
  product exports exactly `name@GLIBC_2.2.5`; an old-style versioned reference links and binds every name, a plain
  `yp_match` reference does not link; a static single-threaded driver over the product's generated bodies checks every
  row (value, errno sentinel / EOPNOTSUPP), stderr text, heap bytes in use (mallinfo2 in the child) and the open
  descriptor set (BTreeSet) before/after.
- Differences row after the RPC client/server row; plants file `~/work/legacy-rpc-libnsl-placeholder/plants.txt` (7).

## Open / informational
- The dynamic path (an old program actually loading libnsl.so.1) cannot run before Liella's ld.so (DL-14); the link
  and readelf checks stand in for it (NOT VERIFIED at run time).
- `docs/spec-symbol-gaps.tsv` lists the 121 names as missing until it is regenerated (`cargo xtask symcov --tsv`); see the
  report for whether this run regenerated it.

## Things not to redo
- Probing the reference's NIS+ calls with a NULL object pointer crashes the probe (caller error); pass real objects or skip.
- A test that measures the heap must make stdio allocate the stream buffers first (stderr's first write allocates).
- Compat entries in libc.so.6 need `crate::internal_entry!` and `cargo xtask compat-gen`; libnsl names are not libc.so.6's.

## Next steps (in order)
1. ~~RPC client/server stubs~~ (D-29.5). 2. ~~libnsl.so.1 placeholder~~ (D-29.6).
3. Legacy crate removal (`liella_legacy::sunrpc_clnt`/`sunrpc_svc`, des, rcmd; move the printer texts and `resv_ports`
   into boundary first). 4. Docs.

## Plan rows to read
`grep -n "^| D-29.[2456] " plan/07-decisions/s26-29.md`; `grep -n "^| D-27.9 " plan/07-decisions/s26-29.md`;
plan/scope-review-legacy.md R10, R11, R13.
