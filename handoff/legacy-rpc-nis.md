# Handoff: LEGACY-RPC-NIS-STUBS (Sun RPC, NIS/NIS+, secure-RPC auth, rcmd become failing stubs)

**State:** branch `task/legacy-rpc-nis-stubs`. The gated commit is 08c2f77 (docs only); this handoff and the report are
committed on top of it. **Story NOT done: RED, because one command was refused.** The inventory step (brief step 1) was
refused, and the stubs, the decision table and the code removal all depend on that inventory, so I stopped there.

## Done
- 08c2f77 `docs/PRINCIPLES.md` §2: added the owner's principle ("When glibc drops or deprecates something, that is a
  strong reason to make an active decision, not the decision itself"). This is brief step 3, third bullet.
- Gate on mei:~/w/s3, job `legacy-rpc-nis-stubs-gate` (`cargo xtask static && cargo xtask affected --run --base
  origin/next`): rc 0, 45 s, peak RSS 362 MB. Clippy in the foreground (`cargo clippy --workspace --all-targets
  --all-features -- -D warnings`): rc 0.
- No plants were run, because there is no new code to plant against.

## REFUSED (exact command, auto-mode classifier; no reason given)
```
cd /home/claude/w/LEGACY-RPC-NIS-STUBS && W=/home/claude/src/liella-ops/work/LEGACY-RPC-NIS-STUBS; ls -d /home/claude/src/liella-ops/work >/dev/null 2>&1 && mkdir -p $W; awk -F'\t' '$1=="libc.so.6"{print $2"\t"$3"\t"$4}' spec/abi/versions.tsv | grep -P '^_*(clnt|clntudp|clnttcp|clntraw|clntunix|callrpc|svc|svcudp|svctcp|svcraw|svcunix|registerrpc|pmap_|rpcb|bindresvport|get_myaddress|getrpcport|authunix|authnone|authdes|key_|netname2|user2netname|host2netname|getnetname|xencrypt|xdecrypt|passwd2des|ecb_crypt|cbc_crypt|des_setparity|rcmd|rexec|rresvport|ruserok|iruserok|ruserpass|_authenticate|_seterr_reply|xprt_|_rpc_|__rpc|getpublickey|getsecretkey|_null_auth|rpc_createerr|svcfd|svcerr|_create_xid|getrpcent|authdes|__key|_des_crypt|des_crypt|svc_)' | sort > $W/inv.tsv; wc -l < $W/inv.tsv; cut -f1,2 $W/inv.tsv | awk '{printf "%s@%s ", $1, $2}'; echo
```
The likely cause is the `mkdir`/write into `~/src/liella-ops/work/` (the thread_common `$S/work/<task-id>/` scratch rule),
a directory that the brief itself does not name. I did not retry the command or route around it.

## Findings so far (read-only survey, for the next run)
- Protocol code: `crates/legacy/src/{sunrpc.rs (220), sunrpc_clnt.rs (378), sunrpc_svc.rs (677), sunrpc_rec.rs (530),
  rcmd.rs (752), des.rs (302)}`. `nis.rs` is a 3-line placeholder ("Not yet implemented"), so libnsl has no code today.
  Check whether `sunrpc_rec.rs` is the XDR record stream (`xdrrec_*`, kept) before removing anything.
- Exports: `crates/boundary/src/posix/s29_rpc_auth.rs` (DES, key_*, authdes, netname, authunix: 27 fns),
  `s29_rpc_clnt.rs` (authnone, pmap_getport, clntudp/clnttcp, bindresvport, clnt_*err*: 14),
  `s29_rpc_svc.rs` (svcudp/svctcp/svcfd, xprt_*, svc_*, _authenticate, _seterr_reply: 18); `s29_xdr_streams.rs` is
  kept. All of these are `liella_compat_*` and get their old versions from `spec/abi/compat-plan.tsv`, through
  `cargo xtask compat-gen` into `csys/src/machine/compat_aliases.S` (a generated file: regenerate it, never edit it).
- `spec/abi/versions.tsv`: 127 `libnsl.so.1` rows. The in-scope libc.so.6 rows still need listing: the refused step.
- Tests to remove or rewrite: `crates/boundary/tests/s29_rpc_*` (for example `s29_rpc_clnt_tcp.rs`).

## Decisions in force
None new. The owner's ruling of 2026-10-27 is in the brief. The §29 D-row has not been written yet.

## Exact next step
Build the inventory again in an allowed scratch place (the coordinator should name it in the brief, for example
`/tmp/lrns/` or `mei:~/work/legacy-rpc-nis-stubs/`). Then do split (a): the stub table with one failure rule per family,
the §29 D-row, the differences row (no restore option, because of security), and a table-driven stub test with plants.
After that, split (b) code removal plus the COMPAT-12 withdrawal, and split (c) RELEASE-NOTES, README and the symcov
regeneration.

## Plan rows to read
`grep -rn "^| D-29\." plan/07-decisions/`, the COMPAT-12 entry in `docs/compat-flags.md`, and `docs/codemap/README.md` §29.
