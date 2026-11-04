# Handoff: LEGACY-RPC-NIS-STUBS-7 (fs-order CI red fixed; rcmd family and auth stubs done; RPC clnt/svc stubs next)

**State:** branch `task/legacy-rpc-nis-stubs-7` (continues -6/-5/-4/-3/-2). Code head **4174064d**; the commit after it
is this handoff and the report. Role **mei:~/w/s2** (synced at 4174064d). origin/next (432f1ef7) merged in at 53a988e7.
**Decisions in force:** D-15.7 (rcmd family = stubs; no compat flag, owner ruling #2292; ruserpass/__ivaliduser stubbed,
#2293), D-29.4 (failing stubs: -1, errno EOPNOTSUPP). Differences row after the secure-RPC row.

## Done this run
- Cause of -6's CI red: the `fs-order` lint (it landed on next after -6 was cut) flagged `open_fds()` in
  `crates/boundary/tests/s29_rpc_auth_stubs.rs`, which counted `/proc/self/fd` with `.map(Iterator::count)`.
  Fix: `open_fds()` collects the descriptor names into a `BTreeSet<OsString>` and the test compares sets. This removes
  the order dependence and also catches a stub that closes one descriptor and opens another. The lint reads only
  the function body, so the set has to be named in the body (`collect::<BTreeSet<_>>()`); a type that appears only
  in the signature does not count (that was gate 1's red, 4174064d fixes it).
- Merge conflict in `plan/07-decisions/s26-29.md`: next added D-29.3 at the place this branch has D-29.4. Kept both, in id order.
- Gate `legacy-rpc-nis-stubs-7-gate2` (mei, 4174064d, `~/work/legacy-rpc-nis-stubs-7/check.sh` under `~/bin/gate 16G`:
  clippy --all-targets --all-features -D warnings, `cargo xtask static`, `cargo xtask affected --base origin/next --run`):
  **rc 0, 6386 passed / 0 failed / 1114 ignored, 4 suites ok, fs-order ok, peak 13G, oom_kill 0.** Superseded: `-gate` (static red, see above).
- Plants `~/work/legacy-rpc-nis-stubs-7/plants.sh <copy-name>` (RAM copy under `/mnt/zram/claude/legacy-rpc-nis-stubs-7/`,
  `cargo test -p liella-boundary --test s29_rpc_auth_stubs`): getnetname and key_setnet each leak a `dup(2)`.
  **`legacy-rpc-nis-stubs-7-plants2` (final head) rc 0, 2/2 CAUGHT** with `<name>: opened a descriptor`. Superseded: `-plants`
  (same result, on the older head 2e0a8012).

## Things not to redo
- Compat entries need `crate::internal_entry!` (internal names) or `export!` (public names).
- Role dirs are not git repos. sync_role.sh takes the host NAME (`mei`), not the address. Default ssh key reaches mei.
- `s15_sockets_symbols` is a liella-boundary test target; `s15_sockets_clauses` is liella's (it needs `cargo xtask so`).
- fs-order: put the ordering token (`BTreeSet`, `.sort`, `.count()` ...) in the body of the function that lists the directory.
- Dead code still to review (clippy is clean, so it is either used or allowed): netaddr::rresvport/rcmd_af/rexec_af/
  ruserok/iruserok/ruserpass/ivaliduser wrappers and helpers; legacy crate `rcmd` (step 3).

## Next steps (in order)
1. RPC client/server stubs (s29_rpc_clnt.rs keeping CreateErrC, createerr storage, __rpc_thread_createerr and the
   clnt_*err printers; s29_rpc_svc.rs; drop runtime.rs compat_from_start; remove the s29_rpc_{clnt,clnt_tcp,svc,svc_tcp,
   interop,reply_source} tests; withdraw COMPAT-12). Size L on its own: give it a full run.
2. libnsl.so.1 (needs DL-1). 3. Legacy crate removal (sunrpc_clnt/svc, des, rcmd). 4. Docs.

## Plan rows to read
`grep -n "^| D-15.7 " plan/07-decisions/s15-sockets-and-network-interfaces.md`; `grep -n "^| D-29.4 " plan/07-decisions/s26-29.md`;
plan/scope-review-legacy.md R10, R11, R13.
