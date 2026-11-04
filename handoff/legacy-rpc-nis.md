# Handoff: LEGACY-RPC-NIS-STUBS-2 (WIP, checkpoint: context budget reached)

**State:** branch `task/legacy-rpc-nis-stubs-2`, code head 93bf059 (+ this handoff commit). Merged run 1 (08c2f77 PRINCIPLES §2 kept).
Role shiki:~/w/sh1. Clippy (workspace, all targets/features, -D warnings) rc 0 and `cargo xtask static` rc 0 on 93bf059.

## Done (slice of split (a): auth family only)
- `crates/boundary/src/posix/s29_rpc_auth.rs` rewritten: all 27 secure-RPC/keyserv/netname/DES/authunix exports are
  failing stubs (table in D-29.3), errno EOPNOTSUPP, outputs untouched, no unsafe.
- Tests: removed `s29_rpc_auth.rs`, `s29_xencrypt.rs`; added table-driven `s29_rpc_auth_stubs.rs` (value, errno, canary
  buffer, no new fd). D-29.3 row (plan/07-decisions/s26-29.md); differences row (no restore option, security).
- Owner ruling 2026-10-27 supersedes ruling #1675 (DES kept for old keys): recorded in D-29.3.

## Gate: RED, not yet diagnosed
`shiki:legacy-rpc-nis-stubs-2-gate2` rc 1 (~/logs/legacy-rpc-nis-stubs-2-gate2.log): failures in liella_cc_lm,
native_v1, s00_conventions_clauses (product-build tests). First suspicion: the first gate (stopped with kill-mine,
superseded) died mid-build and left target/liella products stale/truncated; rerun `cargo xtask so` then the gate.
If still red, check whether anything links `liella_compat_*` auth names expecting old behaviour. Plants NOT run.

## Inventory (scratch, rebuild with spec/abi/versions.tsv + compat-plan.tsv)
106 libc.so.6 names in scope + 127 libnsl.so.1. compat-gen exports `liella_compat_<name>` automatically at the compat
version when the fn exists (xtask/src/compat.rs entries_from): stubs for missing names (callrpc, clnt_create,
registerrpc, pmap_set/unset/getmaps/rmtcall, svcraw/svcunix*, clntraw/clntunix, get_myaddress, getrpcport,
clnt_broadcast, svcudp_enablecache, __rpc_thread_svc_pollfd/max_pollfd) only need the fn defined. Objects
(rpc_createerr, svc_fdset, svc_pollfd, svc_max_pollfd, svcauthdes_stats, __key_*_LOCAL) are not exported today.
rcmd family is public default-version in s15_sockets.rs -> netaddr.rs; tests in s15_sockets_symbols.rs,
core/tests/s15_sockets_clauses.rs, userns_paths.rs (bindresvport), core/tests/oracle/netaddr_diff.c.
libnsl.so.1 is not built at all (needs a new product, DL-1).

## Next step
1. Fix the gate (above); plants on s29_rpc_auth_stubs (a stub returning success, setting another errno, writing the
   buffer, opening a socket) in a RAM copy. 2. Client/server stubs: rewrite s29_rpc_clnt.rs keeping CreateErrC,
   createerr storage, __rpc_thread_createerr and the clnt_*err printers; s29_rpc_svc.rs; drop runtime.rs
   compat_from_start; remove s29_rpc_{clnt,clnt_tcp,svc,svc_tcp,interop,reply_source} tests; withdraw COMPAT-12.
3. rcmd stubs. 4. libnsl. 5. legacy crate removal (sunrpc_clnt/svc, des, rcmd; check sunrpc_rec is xdrrec first). 6. docs.
