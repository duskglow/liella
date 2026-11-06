# Handoff: LEGACY-RPC-CLNT-SVC-STUBS (RPC client/server are failing stubs; lane step 1 done)

**State:** branch `task/legacy-rpc-clnt-svc-stubs` (cut from origin/next 31ecf574). Code commits 1afb91ab (stubs, docs,
D-29.5, tests) and the compat-gen regeneration commit after it; then this handoff and the report. Role **keke:~/w/r5**.
**Decisions in force:** D-29.5 (new: client, port mapper and server are failing stubs; supersedes D-29.2), D-29.4
(secure-RPC/DES stubs), D-15.7 (rcmd family). Differences row for the stubs after the secure-RPC and rcmd rows.

## Done this run (lane "Next steps" item 1)
- `crates/boundary/src/posix/s29_rpc_clnt.rs`, `s29_rpc_svc.rs`: every client, port mapper and server entry fails at once,
  `errno` `EOPNOTSUPP`, no descriptor/port mapper/memory; constructors and client-side calls also set `rpc_createerr`
  `RPC_SYSTEMERROR`/`EOPNOTSUPP`. Kept: `CreateErrC` + per-thread createerr storage, `__rpc_thread_createerr`, the
  `clnt_*err` printers, `__rpc_thread_svc_fdset` (always-empty set), `bindresvport`, `_rpc_dtablesize`. Failure table: D-29.5.
- 17 names that were not exported (`clnt_create`, `callrpc`, `pmap_set` ... see D-29.5) are now exported as stubs; removed from
  `spec/abi/compat-missing.tsv`; `csys/src/machine/compat_aliases.S` regenerated (`cargo xtask compat-gen`).
- `runtime.rs` `compat_from_start` call, the `liella.compat.rpc_reply_any_source` tunable (config.rs + its unit test + the
  s01 startup literal) removed; COMPAT-12 row status "withdrawn" (id stays: ids must be contiguous); D-29.2 marked superseded.
- Tests removed: `crates/boundary/tests/s29_rpc_{clnt,clnt_tcp,svc,svc_tcp,interop,reply_source}.rs`. New:
  `crates/boundary/tests/s29_rpc_clnt_svc_stubs.rs` (one #[test] on purpose: it compares `mallinfo2().uordblks`, which
  another test thread would move; also descriptor sets as a BTreeSet, canary buffers, rpc_createerr, printer text).
- Checks on keke r5 (job `legacy-rpc-clnt-svc-stubs-prep2`, rc 0): compat-gen --check, `cargo xtask clippy`,
  `cargo xtask static`, the new test (1 passed). Superseded: `-prep` (test closures did not compile; fixed).
- Gate `legacy-rpc-clnt-svc-stubs-gate` (affected --base origin/next --run under ~/bin/gate 16G) and plants
  `legacy-rpc-clnt-svc-stubs-plants` (7 plants, `~/work/legacy-rpc-clnt-svc-stubs/plants.txt`, all with `msg:`): see the
  report for their results.

## Open questions / informational
- `__libc_clntudp_bufcreate`, `__rpc_thread_svc_pollfd`, `__rpc_thread_svc_max_pollfd` stay unexported (internal, no
  documented contract); objects `rpc_createerr`, `svc_fdset`, `svc_pollfd`, `svc_max_pollfd`, `svcauthdes_stats` are LG-6's.
- `docs/RELEASE-NOTES.md` (past release text) still says COMPAT-12 is implemented; left as history, the next notes should say it was withdrawn.
- Dead code for step 3: `liella_legacy::sunrpc_svc` (only `RpcErr`/rules no longer used by boundary), most of
  `sunrpc_clnt` (boundary still uses `sperrno`, `sperror`, `spcreateerror`, `Text`, `resv_ports`), `users::RPC_*` blocks still used.

## Things not to redo
- Compat entries need `crate::internal_entry!`; a new compat name also leaves compat-missing.tsv and needs `cargo xtask compat-gen`.
- No local cargo on the dispatcher host: run compat-gen on the role and copy the generated `.S` back.
- Tests of liella-boundary use no `unsafe`: read C results through safe helpers (`create_error`, `create_error_text`).
- fs-order: the ordering token goes in the body of the function that lists the directory.

## Next steps (in order)
1. ~~RPC client/server stubs~~ (done here, D-29.5).
2. libnsl.so.1 placeholder (needs DL-1). 3. Legacy crate removal (sunrpc_clnt/svc, des, rcmd; move the printer texts and
   `resv_ports` into boundary first). 4. Docs.

## Plan rows to read
`grep -n "^| D-29.[245] " plan/07-decisions/s26-29.md`; `grep -n "^| D-15.7 " plan/07-decisions/s15-sockets-and-network-interfaces.md`;
plan/scope-review-legacy.md R10, R11, R13.
