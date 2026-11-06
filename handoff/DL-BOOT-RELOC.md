# DL-BOOT-RELOC (phase 14 DL-4): self-relocator, first used by libc.a's static-PIE start

**State:** branch `task/dl-boot-reloc-5` (continues dl-boot-reloc-4; base origin/next), code head `6ae9fced`
(unchanged from DL-BOOT-RELOC-4; this run added handoff commits only). Role shiki:~/w/sh2.
**Verdict: DO NOT LAND until the RPC test story fixes `s29_rpc_clnt_svc_stubs`** (the CI red is that
test's own race, present on origin/next without this branch; evidence below). The branch itself needs no change.

## DL-BOOT-RELOC-5: the CI red (train 502, tip 4cb906564)
* Failure: `liella-boundary::s29_rpc_clnt_svc_stubs every_rpc_client_and_server_stub_fails_cleanly`,
  `svcudp_create: kept memory` +1024 bytes, in CI's second test build only (`--features
  liella-boundary/export,liella-boundary/selftest`); the default-feature build passed in the same run.
* **Reproduced on origin/next alone** (job shiki `dl-boot-reloc-5-repro1`, rc 0; evidence only): the test binary
  built with those features from origin/next (sha256 faf0d568...) and from origin/next + this branch
  (bae174cd..., a no-commit merge in a scratch worktree, never pushed), each run 26 workers x 150 = 3900 times
  with `LIELLA_TEST_MALLOC_PERTURB=165` (as CI), host load 4.6-8.3 on 26 CPUs (Haswell):
  **next 20 / 3900 fail, next+branch 24 / 3900 fail.** Same signature both: +1024 (50496 -> 51520) or +176
  (51344 -> 51520) bytes, at a random stub each time (callrpc, svcraw_create, svc_getreq, svcudp_create,
  svc_unregister, _authenticate, svc_sendreply, ...). A deterministic lazy allocation in a stub would hit the
  same stub every time and be absorbed by the warm-up round.
* Cause (test side): with `export`, the test executable defines Liella's `malloc`, so Rust std / libtest
  allocations land in Liella's heap, and `mallinfo2().uordblks` is process-wide. libtest runs the test on a
  spawned thread (the panic names thread `every_rpc_client_and_server_stub_fails_cleanly`) while its main thread
  still allocates after the spawn; under load the main thread is preempted past the warm-up, so its allocation
  lands inside the measured window. `--test-threads=1` does not help (job shiki `dl-boot-reloc-5-repro2`, rc 0,
  evidence only: 15 / 3900 fail, still on the spawned thread).
* Not this branch: the code this branch adds (`runtime.rs` PIE path, `bootstrap.rs`) is `cfg(feature = "runtime")`
  and is not compiled into that test build at all.
* Recommendation for the RPC test story: run the heap check in a process with no other thread (a
  `harness = false` test target, or the repo's re-exec helper with the check on the child's main thread), or a
  per-thread allocation count; do not loosen the equality.

## The three dead-code warnings (classed::start, nscache::start, sys::set_sysinfo_ehdr)
* Not from this branch: the same three warnings appear in three CI gate logs on mei that do not contain this
  branch (ci-gate-34c98f61c, -435b9fc3a, -31ecf574c: each lists sys.rs:299, classed.rs:200, nscache.rs:49, and no
  bootstrap.rs). Their only caller is `runtime::start` (runtime.rs:280, 357, 388), which exists only with feature
  `runtime`; a test build without it (export,selftest) compiles them uncalled.
* This branch does not change which start-up init runs: `start::<false>` (liella-crt1.o) and `start::<true>`
  (liella-rcrt1.o) both run all three on the same path as origin/next (classed::start still only for a static
  program). Nothing to wire or remove; the tidy fix is `#[cfg_attr(not(feature = "runtime"), allow(dead_code))]`
  on the three, the pattern ffi.rs already uses (not done here: the brief's case 3 stops the run).

## Jobs this run (shiki)
* `dl-boot-reloc-5-lint0`: `cargo xtask clippy && cargo xtask static` on the inherited head 6ae9fced, rc 0.
* `dl-boot-reloc-5-lint1`: the same on the final head (code identical; handoff commits only).
* `dl-boot-reloc-5-repro1`, `dl-boot-reloc-5-repro2`: evidence loops above (scripts in ~/work/DL-BOOT-RELOC-5/).

## Done earlier (DL-BOOT-RELOC-4, -3; see their reports in handoff/reports/)
* -4: origin/next merged (so.rs keeps the DL-PLACEHOLDERS build and liella-rcrt1.o whole; D-27.9/10/11 once each);
  docs/toolchain.md documents liella-rcrt1.o (ruling #3670); FULL gate rc 0 (11128 passed, conformance 124/0).
* -3: link-time selection (`liella-rcrt1.o` -> `start::<true>`; `__libc_start_main` -> `start::<false>`, no
  runtime test on the non-PIE start); relr-span and tls-rebase plant fixes; plants 24/24 CAUGHT on 79821fcf.
* Note: docs/toolchain.md 3.3 still describes the old libpthread-only placeholders (D-27.9's text to fix).

## Open / next
1. Coordinator: open the story on `s29_rpc_clnt_svc_stubs` (above); once it lands, this branch can go to CI
   again with no change (merge origin/next first only if CI reports a conflict).
2. Optional tidy: the cfg_attr allow on the three runtime-only functions.
3. Optional: a conformance check that RELRO is read-only in a static PIE.
4. PT_PHDR load offset: already cut as STATIC-PIE-LOAD-OFFSET; not here.

Decisions: D-27.10. Plan rows: design §0a (item 9), §1; phase-14 item 8; R14-1. Plants: handoff/DL-BOOT-RELOC-plants.txt.
