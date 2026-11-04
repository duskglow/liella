# Handoff: LEGACY-RPC-NIS-STUBS-6 (rcmd family stubbed and verified: gate green, plants run; RPC clnt/svc stubs next)

**State:** branch `task/legacy-rpc-nis-stubs-6` (continues -5/-4/-3/-2). Code head **a51b0a96** (s15_8_c_3 errno fix);
the commit after it is this handoff and the report. Role **mei:~/w/s2** (synced at a51b0a96).
**Decisions in force:** D-15.7 (rcmd family = stubs; no compat flag, owner ruling #2292; ruserpass/__ivaliduser stubbed,
#2293), D-29.4 (failing stubs: -1, errno EOPNOTSUPP). Differences row after the secure-RPC row.

## Done this run
- Why -5's gate2 was red: `s15_8_c_3` set errno to 0 *before* the `ivalid` fixture helper, whose own file I/O
  (scratch path, `fs::write`, `fopen`) left ENOENT (2) behind; the stub (`__ivaliduser` = bare `-1`) never writes
  errno. Fixed in the test (a51b0a96): `ivalid` plants a non-zero sentinel (0x5a5a) immediately before the call and
  returns errno read immediately after; the ruserok/iruserok part uses the same sentinel (a stub that clears errno
  is now caught too). The test already cites D-15.7. No product code changed.
- Gate `legacy-rpc-nis-stubs-6-gate` (mei, a51b0a96, `~/work/legacy-rpc-nis-stubs-6/check.sh` under `~/bin/gate 16G`:
  clippy --all-targets --all-features -D warnings, `cargo xtask static`, `cargo xtask affected --base origin/next --run`):
  **rc 0, 6350 passed / 0 failed / 1114 ignored, 4 suites ok, peak 13G, oom_kill 0.**
- regex_enomem (`regcomp_out_of_memory_returns_reg_espace_and_releases_its_work`) **PASSES** in that gate on mei
  under `gate 16G`, both modes. The -4 failure is not reproduced on keke or on mei: treat as closed unless it recurs.
- Plants: `~/work/legacy-rpc-nis-stubs-6/plants.sh <copy-name>` (RAM copy `/mnt/zram/claude/legacy-rpc-nis-stubs-6/<name>`,
  runs `cargo test -p liella --test s15_sockets_clauses`, CAUGHT needs `test <name> ... FAILED`, exit 1 on any survivor).
  10 plants on crates/boundary/src/posix/s15_sockets.rs: EINVAL instead of EOPNOTSUPP, rresvport writes *port,
  rresvport_af returns an fd, rcmd writes *fd2p, ruserok allows, iruserok_af allows, __ivaliduser clears errno,
  __ivaliduser allows, rexec sets no errno, ruserpass writes *aname. Result: **`legacy-rpc-nis-stubs-6-plants4` rc 0,
  10/10 CAUGHT** with the expected test (s15_8_c_1 for rcmd, _2 for rresvport, _3 for the trust checks, _4 for rexec/ruserpass).
  Superseded: `-plants` (baseline named `s15_sockets_symbols`, a liella-boundary target), `-plants2` (baseline red: the suite
  needs `cargo xtask so`), `-plants3` (10/10 caught, but the rcmd plant's expected name was wrong: _1, not _2).

## Things not to redo
- Compat entries need `crate::internal_entry!` (internal names) or `export!` (public names).
- Role dirs are not git repos. sync_role.sh takes the host NAME (`mei`), not the address. Default ssh key reaches mei.
- `s15_sockets_symbols` is a liella-boundary test target; `s15_sockets_clauses` is liella's.
- Dead code still to review (clippy is clean, so it is either used or allowed): netaddr::rresvport/rcmd_af/rexec_af/
  ruserok/iruserok/ruserpass/ivaliduser wrappers and helpers; legacy crate `rcmd` (step 4).

## Next steps (in order)
1. RPC client/server stubs (s29_rpc_clnt.rs keeping CreateErrC, createerr storage, __rpc_thread_createerr and the
   clnt_*err printers; s29_rpc_svc.rs; drop runtime.rs compat_from_start; remove the s29_rpc_{clnt,clnt_tcp,svc,svc_tcp,
   interop,reply_source} tests; withdraw COMPAT-12). Size L on its own: give it a full run.
2. libnsl.so.1 (needs DL-1). 3. Legacy crate removal (sunrpc_clnt/svc, des, rcmd). 4. Docs.

## Plan rows to read
`grep -n "^| D-15.7 " plan/07-decisions/s15-sockets-and-network-interfaces.md`; `grep -n "^| D-29.4 " plan/07-decisions/s26-29.md`;
plan/scope-review-legacy.md R10, R11, R13.
