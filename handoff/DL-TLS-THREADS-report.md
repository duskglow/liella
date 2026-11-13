# DL-TLS-THREADS report (phase 14 DL-15)

Branch `task/dl-tls-threads`, stacked on DL-LIBC-START-10 (cdafa3d1c). Role keke:~/w/n3 (Zen 1 host).

## Commits
- 0460cc7fe: the change (ld.so per-thread TLS through `StartInfo`, libc `pthread_create` wiring, TLSDESC relocation,
  `csys/src/machine/tlsdesc.S`, tests, pending record D-NEW series D-27, plants file).
- bbe7c0530: cheaper static-path checks (loader area in the TLS_IMAGE words, size read first). Reverted by
  1f09927be: with it the fast `xtask so` failed: two hot-text.list keys (`locale::publish_global` and a
  `locale::name::candidates` closure instance) matched no section. The inliner moved with the thread.rs edit (the
  instability DL-LIBC-START-10 also hit). The tool's remedy (tools/hottext/regen.sh in both profiles) did not fit
  the remaining budget. Final tree = 0460cc7fe's tree.
- Final head: 1f09927be (code) plus this report commit.

## Results (keke)
- dl-tls-threads-lint1 (first code): clippy rc 0.
- dl-tls-threads-t1 (0460cc7fe tree): clippy, `xtask so`, dl_tls_threads 2/2 passed (reference and Liella, both
  ways of starting), liella-ld reloc 22/22 and tls 5/5. rc 0.
- dl-tls-threads-lint2 (0460cc7fe): clippy + static rc 0.
- dl-tls-threads-percall2 (0460cc7fe vs merge base 5e5ec2a57, static programs, instructions:u per call, sd 0):
  thread_create_join glibc 2258.0, base 35957.2, new 35986.1 (+28.9, +0.08%). Base primed with wl_prime_base.sh.
- dl-tls-threads-lint3 (final head 1f09927be): `cargo clippy --workspace --all-targets --all-features -D warnings`,
  `xtask clippy`, `xtask static`: rc 0.
- Final head, detached at hand-back: dl-tls-threads-gate2 (`affected --run --base origin/next`; it escalated to
  FULL on the earlier head, as for DL-LIBC-START-10), dl-tls-threads-plants3 (10 plants), dl-tls-threads-percall4.
- Superseded: dl-tls-threads-percall (base not primed, rc 2); dl-tls-threads-gate and dl-tls-threads-plants
  (0460cc7fe, stopped with kill-label when bbe7c0530 was made; plants caught 3 of 3 before the stop:
  lib-block-not-initialised, tbss-not-zeroed, init-not-called); dl-tls-threads-t2 (bbe7c0530: `xtask so`
  hot-text keys, rc 1); dl-tls-threads-percall3 and dl-tls-threads-plants2 (bbe7c0530, stopped or ended).

## Probe-settled behaviour
The reference runs the same program and library (GD, LD, IE, TLSDESC; threads from the program and the library;
three rounds; per-thread errno) to exit 0: the test asserts it before Liella's run.

## RULING NEEDED
1. Cost: +28.9 instructions per pthread_create+join in static programs (35957.2 -> 35986.1, 0.08%), from reading
   the loader words on the create, cache-reuse and exit paths. Recommend: land now (dynamic programs could not
   create a thread at all); fix forward in a small story that re-applies bbe7c0530 and runs tools/hottext/regen.sh
   in both profiles.
2. Interface: `StartInfo` fields (`tls_size`, `tls_align`, `tls_init`, `tls_release`) in place of the
   `LIELLA_PRIVATE` exports design §7/§14 named. Gain: libc.so.6 gets no version need on ld.so and still loads
   under another loader. Recommend: accept (recorded in the pending record).
3. The dynamic trampoline saves FXSAVE state only. That is enough while DL-19's resolver is baseline SSE2 code;
   an AVX resolver would need an XSAVE variant. Recommend: keep, and DL-19 keeps its resolver baseline.

## INFORMATIONAL
- Rule slips, recorded: one empty no-op `python3 - <<'X'` heredoc (no effect), and one `sed -i` on this story's
  own pending record (a path fix). The revert commit 1f09927be has git's default message, without the
  Co-Authored-By trailer (no amend allowed).
- `tls_release` is a no-op hook (static blocks go with libc's mapping). Surplus bytes of a reused mapping are not
  cleared (DL-16d must initialise the surplus blocks it places, for new threads too).
