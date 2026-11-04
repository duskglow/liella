# Handoff: LEGACY-RPC-NIS-STUBS-5 (rcmd family stubbed; clippy+static clean; direct gate running; no plants yet)

**State:** branch `task/legacy-rpc-nis-stubs-5` (continues -4/-3/-2). Code head **4ec40d9b** (ab60300d rcmd stubs + 4ec40d9b drop unused EACCES); the commit after it is this handoff.
Role **keke:~/w/r5** (synced at 4ec40d9b).
**Results:** regexcmp rc 0: **10/10 PASS** (5 on origin/next, 5 on the branch, keke, plain jobrun, not under
`~/bin/gate`); the -4 failure on mei (under gate 16G) is NOT reproduced and NOT explained. Gate `legacy-rpc-nis-stubs-5-gate`
(ab60300d) rc 1: clippy, unused `EACCES` (fixed in 4ec40d9b). Gate **`legacy-rpc-nis-stubs-5-gate2`** (4ec40d9b, same
script): clippy and static passed; `cargo xtask affected` was running at hand-back (result not seen).
Jobs on keke:
- `legacy-rpc-nis-stubs-5-regexcmp` (script `~/work/legacy-rpc-nis-stubs-5/regex_cmp.sh`): runs
  `liella-boundary::regex_enomem regcomp_out_of_memory_returns_reg_espace_and_releases_its_work` in the
  exports-interposed mode (`--features liella-boundary/export,liella-boundary/selftest`, as `xtask affected` pass 1)
  5x on **origin/next 5a695260** (RAM copy `/dev/shm/claude/legacy-rpc-nis-stubs-5/next`) and 5x on the role (this
  branch). Per-run logs `~/work/legacy-rpc-nis-stubs-5/run-{next,r5}-<n>.log`; PASS/FAIL lines in
  `~/logs/legacy-rpc-nis-stubs-5-regexcmp.log`. Fails on next too -> separate defect (report, not ours); only on the
  branch -> fix here. NOTE: the role was synced to ab60300d before this job started, so its "branch" runs test ab60300d.
- `legacy-rpc-nis-stubs-5-gate` (script `~/work/legacy-rpc-nis-stubs-5/check.sh`, under `~/bin/gate 16G`): waits for
  regexcmp, then `cargo clippy --workspace --all-targets --all-features -D warnings`, `cargo xtask static`,
  `cargo xtask affected --base origin/next --run`; prints `RED <step>` or `GREEN`.

## Done this run
- Applied the -4 rcmd patch (patch file removed) and finished it: comments cite D-29.4/D-15.7; `ruserpass` stubbed
  (-1, EOPNOTSUPP, outputs untouched) and `__ivaliduser` stubbed (-1, errno untouched), per coordinator #2293.
- Tests: s15_sockets_clauses.rs s15_8_c_3 (trust-line part now asserts the deny stub, ftell 0 = stream unread; the
  `rcmd::iruserok` legacy-crate part kept) and s15_8_c_4 (rexec/rexec_af/ruserpass stub asserts; `read_z`/`fake_rexecd`
  deleted); s15_sockets_symbols.rs sym_iruserok(_af), sym___ivaliduser, sym_rexec(_af) (`rexecd` deleted),
  sym_rresvport(_af), sym_ruserok(_af), sym_ruserpass rewritten; `const EOPNOTSUPP: i32 = 95;` added.
  userns_paths.rs: rresvport dropped (outside and in-namespace), bindresvport kept.
- D-15.7 row (plan/07-decisions/s15-sockets-and-network-interfaces.md) and a differences row (after the secure-RPC row).
  Rulings applied: #2292 no compat flag for rexec (and none for the family); #2293 ruserpass/__ivaliduser stubbed.

## Compile fallout (clippy -D warnings is now clean; kept for the dead-code review)
- Unused now: netaddr::rresvport/rcmd_af/rexec_af/ruserok/iruserok/iruserok_v4/ruserpass/ivaliduser and their helpers;
  in s15_sockets.rs maybe `strz`/`conv` imports; in s15_sockets_symbols.rs maybe `EACCES`, `EAFNOSUPPORT`, `s17`,
  `in_child_env`, `text_at`; in s15_sockets_clauses.rs check `text_at`, `TrustFiles`, `io`. Delete dead code (legacy
  crate rcmd removal is step 4; netaddr wrappers can go now).
- `errno::set_raw`/`errno::get` and `AF_INET6`/`AF_UNIX` must exist in s15_sockets_clauses.rs (c_1 already used them).
- `cargo xtask static` trace: the `// Trace: s15_8_c_N` lines may need removing if trace complains.

## Things not to redo
- Compat entries need `crate::internal_entry!` (internal names) or `export!` (public names).
- Role dirs are not git repos. sync_role.sh takes the host NAME (`keke`), not the address.
- Check new D-row ids against origin/next (D-15.7 was free on next 5a695260? not re-checked this run: check).

## Next steps (in order)
1. Read gate2 (`~/logs/legacy-rpc-nis-stubs-5-gate2.log` on keke); fix what it finds (run only the failing suite).
   regex_enomem: if it fails again it is mei/gate-specific (passes on next and branch on keke). Then plants on s15_sockets.rs: stub returns a real fd / sets no errno / writes *port /
   ruserok returns 0 / __ivaliduser reads the stream (msg: the s15_8_c_* and sym_* names).
2. RPC client/server stubs (s29_rpc_clnt.rs keeping CreateErrC, createerr storage, __rpc_thread_createerr and the
   clnt_*err printers; s29_rpc_svc.rs; drop runtime.rs compat_from_start; remove the s29_rpc_{clnt,clnt_tcp,svc,svc_tcp,
   interop,reply_source} tests; withdraw COMPAT-12).
3. libnsl.so.1 (needs DL-1). 4. Legacy crate removal (sunrpc_clnt/svc, des, rcmd). 5. Docs.

## Plan rows to read
`grep -n "^| D-15.7 " plan/07-decisions/s15-sockets-and-network-interfaces.md`; `grep -n "^| D-29.4 " plan/07-decisions/s26-29.md`;
plan/scope-review-legacy.md R10, R11, R13.
