# Handoff: LEGACY-RPC-NIS-STUBS-5 (rcmd family stubbed, NOT compiled yet; regex_enomem comparison queued)

**State:** branch `task/legacy-rpc-nis-stubs-5` (continues -4/-3/-2). Code head ab60300d (WIP: rcmd stubs); the
commit after it holds only this handoff and the report. Role **keke:~/w/r5** (synced at ab60300d's tree).
Jobs on keke (both detached, both queued at hand-back, neither result seen):
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

## Likely compile fallout (fix first next run, from the gate log)
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
1. Read the two job logs. Explain regex_enomem (next vs branch) and fix if branch-only. Fix compile/lint fallout,
   re-gate (direct tier), then plants on s15_sockets.rs: stub returns a real fd / sets no errno / writes *port /
   ruserok returns 0 / __ivaliduser reads the stream (msg: the s15_8_c_* and sym_* names).
2. RPC client/server stubs (s29_rpc_clnt.rs keeping CreateErrC, createerr storage, __rpc_thread_createerr and the
   clnt_*err printers; s29_rpc_svc.rs; drop runtime.rs compat_from_start; remove the s29_rpc_{clnt,clnt_tcp,svc,svc_tcp,
   interop,reply_source} tests; withdraw COMPAT-12).
3. libnsl.so.1 (needs DL-1). 4. Legacy crate removal (sunrpc_clnt/svc, des, rcmd). 5. Docs.

## Plan rows to read
`grep -n "^| D-15.7 " plan/07-decisions/s15-sockets-and-network-interfaces.md`; `grep -n "^| D-29.4 " plan/07-decisions/s26-29.md`;
plan/scope-review-legacy.md R10, R11, R13.
