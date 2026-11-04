# Handoff: LEGACY-RPC-NIS-STUBS-4 (CI static red fixed; rcmd stubs half-written, saved as a patch)

**State:** branch `task/legacy-rpc-nis-stubs-4` (continues -3/-2). Code head 3532877 (renumber); the commit after it holds
only this handoff, the report and the refreshed rcmd patch. Role mei:~/w/s2. Job on mei: `legacy-rpc-nis-stubs-4-gate`
(direct tier): **rc 1**, 6265 passed / 1 failed: `liella-boundary::regex_enomem
regcomp_out_of_memory_returns_reg_espace_and_releases_its_work`, failing only in the exports-interposed suite (child exit 14,
regex_enomem.rs:119); it passed in the plain suite. Not diagnosed (budget); the code change since -3's green gate is comments,
docs and a plan-row id only. Explaining it comes first next run (no reruns to pass). Plants: none (no code changed). clippy (`--workspace --all-targets --all-features -D warnings`) and `cargo xtask static` clean on the role;
`cargo xtask static` also clean on the branch merged with origin/next (merge-tree export in scratch, not a git merge).

## The CI red (73eb047): explained and fixed
Merged with next, `D-29.3` was defined twice: next landed its own D-29.3 (`<regexp.h>` step/advance, malloc_*_state;
owner R6/R9) while this branch had D-29.3 for the secure-RPC stubs. Fix 3532877: the secure-RPC row is now **D-29.4**
(plan/07-decisions/s26-29.md, differences row, s29_rpc_auth.rs, s29_rpc_auth_stubs.rs). Old reports (-2/-3 json) still
say D-29.3: historical, left as they were.

## Decisions in force
D-29.4 (secure-RPC/keyserv/netname/DES: 27 failing stubs, EOPNOTSUPP, outputs untouched, no restore option) and its
differences row. Owner rulings 2026-11-04 R10/R11 for rcmd: not yet in a D-row. **D-15.7** is the next free s15 row
(checked against next) and the patch already cites it.

## Things not to redo
- Compat entries need `crate::internal_entry!` (internal names) or `export!` (public names).
- Role dirs are not git repos. sync_role.sh takes the host NAME (`mei`), not the address.
- Check new D-row ids against origin/next (`git merge-tree --write-tree HEAD origin/next`, archive, `cargo xtask static`).

## Next steps (in order)
1. **rcmd family (R10/R11).** `handoff/legacy-rpc-nis-rcmd-stubs.patch` (`git apply`, then delete it) now holds:
   s15_sockets.rs stubs (rresvport(_af)/rcmd(_af)/rexec(_af) `-1` + EOPNOTSUPP, nothing written; ruserok(_af)/
   iruserok(_af) `-1`, errno untouched); rewritten core/tests/s15_sockets_clauses.rs s15_8_c_1 and c_2; boundary/tests/
   s15_sockets_symbols.rs sym_rcmd, sym_rcmd_af, sym___check_rhosts_file, sym___rcmd_errstr. NOT compiled. Still to do:
   - in the patch's s15_sockets.rs comments change `D-29.3` to `D-29.4`; the symbols test needs `const EOPNOTSUPP: i32 = 95;`;
     drop the `// Trace:` lines the patch added to c_1/c_2 if trace complains (the fn names already name the clauses).
   - s15_sockets_clauses.rs s15_8_c_4 (rexec): replace with stub assertions; delete `read_z`/`fake_rexecd`; c_3 calls
     liella_legacy::rcmd directly (keep until the crate goes); check `text_at` still has users.
   - s15_sockets_symbols.rs: sym_rexec, sym_rexec_af (delete `rexecd`), sym_rresvport, sym_rresvport_af to stub asserts;
     sym_ruserok/_af and sym_iruserok/_af already expect -1 (add errno-untouched checks).
   - core/tests/userns_paths.rs: its C program expects rresvport to bind 512..1023 (lines ~77, 98-101, 209-210): drop
     those, keep bindresvport.
   - netaddr::rresvport/rcmd_af/rexec_af/ruserok/iruserok may become unused (dead-code lint).
   - D-15.7 row + one differences row for the family; plants (stub returns a real fd / sets no errno / writes *port).
   - RULING NEEDED: R10 asks for a COMPAT flag for rexec; D-29.4 offers none for the same reason (cleartext password):
     recommend no flag. ruserpass and __ivaliduser are not named in R11: recommend stubbing them with the family.
2. RPC client/server stubs (s29_rpc_clnt.rs keeping CreateErrC, createerr storage, __rpc_thread_createerr and the
   clnt_*err printers; s29_rpc_svc.rs; drop runtime.rs compat_from_start; remove the s29_rpc_{clnt,clnt_tcp,svc,svc_tcp,
   interop,reply_source} tests; withdraw COMPAT-12; names without a fn only need an `internal_entry!` fn).
3. libnsl.so.1 (no product exists; needs DL-1). 4. Legacy crate removal (sunrpc_clnt/svc, des, rcmd; check sunrpc_rec
   is xdrrec first). 5. Docs.

## Plan rows to read
`grep -n "^| D-29.4 " plan/07-decisions/s26-29.md`; plan/scope-review-legacy.md rows R10, R11, R13.
