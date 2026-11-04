# Handoff: LEGACY-RPC-NIS-STUBS-3 (secure-RPC stubs verified; rcmd stubs drafted, not landed)

**State:** branch `task/legacy-rpc-nis-stubs-3` (continues -2), code head 01689f9 (tree ce8f9fb, the one the jobs ran on) (+ this handoff/report commit).
Role kanon:~/w/kn1. Jobs on kanon: `legacy-rpc-nis-stubs-3-so` (rc 0), `legacy-rpc-nis-stubs-3-gate`,
`legacy-rpc-nis-stubs-3-plants`, `legacy-rpc-nis-stubs-3-lint` (results: handoff/reports/LEGACY-RPC-NIS-STUBS-3.json).

## Decisions in force
D-29.3 (secure-RPC/keyserv/netname/DES: 27 failing stubs, EOPNOTSUPP, outputs untouched, no restore option) and its
differences row. Owner rulings 2026-11-04 R10/R11 (liella-ops state/PH14-OWNER-RULINGS.md) for rcmd: not yet in a D-row.

## Run 2's red gate: explained (not stale products)
`cargo xtask so` itself failed: "libc.a does not define the target of 27 compat versions ... liella_compat_authdes_create
...". Run 2 wrote the stubs as plain `pub extern "C" fn` (no `#[no_mangle]`), so no symbol existed; every
product-linking test (liella_cc_lm, native_v1, s00_conventions_clauses, s01 relro) then failed on the missing products.
The unit test calls the Rust fns directly, so it could not see this. Fix 01689f9: every stub is a `crate::internal_entry!`.
A plant (`unexported`) now proves `xtask so` catches a missing wrapper.

## Things not to redo
- Do not write compat entries without `crate::internal_entry!` (internal names) or `export!` (public names).
- Role dirs are not git repos: pass no `--base` inside the role; the sync records it.

## Next steps (in order)
1. **rcmd family (R10/R11).** The export rewrite is drafted (not compiled, not tested):
   `handoff/legacy-rpc-nis-rcmd-stubs.patch` (`git apply` it, then delete it; s15_sockets.rs: rresvport(_af), rcmd(_af),
   rexec(_af) return -1 errno EOPNOTSUPP with nothing written; ruserok(_af)/iruserok(_af) return -1, errno untouched).
   Still needed: rewrite the tests that expect the old behaviour: boundary/tests/s15_sockets_symbols.rs
   (sym___check_rhosts_file, sym_iruserok(_af), sym_rcmd(_af), sym___rcmd_errstr, rexecd/sym_rexec(_af),
   sym_rresvport(_af), sym_ruserok(_af)); core/tests/s15_sockets_clauses.rs s15_8_c_1/2/3 (c_3 calls liella_legacy::rcmd
   directly: keep or drop with the crate); core/tests/userns_paths.rs (its C program expects rresvport to bind
   512..1023: drop those lines, keep bindresvport); oracle/netaddr_diff.c ruserok line still matches (-1).
   One D-row (D-15.8.x) + one differences row for the family. R10 asks for a COMPAT flag for rexec; D-29.3 offers no
   restore for the same reason (cleartext password): RULING NEEDED, recommend no flag, consistent with D-29.3.
   ruserpass and __ivaliduser are not named in R11: ask, or treat as part of the family (recommend stub too).
2. RPC client/server stubs: rewrite s29_rpc_clnt.rs keeping CreateErrC, createerr storage, __rpc_thread_createerr and
   the clnt_*err printers; s29_rpc_svc.rs; drop runtime.rs compat_from_start; remove s29_rpc_{clnt,clnt_tcp,svc,svc_tcp,
   interop,reply_source} tests; withdraw COMPAT-12. Names without a fn (callrpc, clnt_create, registerrpc, pmap_*,
   svcraw/svcunix*, clntraw/clntunix, get_myaddress, getrpcport, clnt_broadcast, svcudp_enablecache,
   __rpc_thread_svc_pollfd/max_pollfd) only need an `internal_entry!` fn (compat-gen binds it).
3. libnsl.so.1 (no product exists; needs DL-1). 4. Legacy crate removal (sunrpc_clnt/svc, des, rcmd; check
   sunrpc_rec is xdrrec first). 5. Docs.

## Plan rows to read
`grep -n "^| D-29.3 " plan/07-decisions/s26-29.md`; plan/scope-review-legacy.md rows R10, R11, R13.
