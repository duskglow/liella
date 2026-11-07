# STATIC-CXX-THROW-PTHREAD-ONCE (handoff, also the full report)

State: branch `task/static-cxx-throw-pthread-once` (from origin/next 66b8d3ba), code head 2e2fe629, role kanon:~/w/kn1.
Result: a static C++ program that throws without threads no longer jumps to address 0.

## Probe (black-box; build host's g++ 13.3, reference static libc 2.39; `nm`, `ld -y`)
* libgcc_eh's weak thread references: unwind-dw2.o `__pthread_key_create`, `pthread_once`; unwind-dw2-fde-dip.o
  `__pthread_key_create`, `pthread_mutex_lock/unlock`, `pthread_cond_wait/broadcast`; emutls.o also
  `pthread_getspecific/setspecific`, `pthread_key_create`.
* Reference, throw/catch, no threads: links `pthread_once`/`__pthread_once` (pulled by a libc member's own strong reference to
  `__pthread_once`) and the mutex functions; NOT `__pthread_key_create`. Runs (rc 0).
* Reference, one thread (std::thread): `pthread_create.o` references `__pthread_key_create`; both it and `pthread_once` linked; runs.
* Liella before: libc.a's one Rust member defines `__pthread_key_create`, getspecific, setspecific, key_delete, mutex
  lock/unlock, pthread_create, ...; `pthread_once`/`__pthread_once`/`call_once` live in csys's machine/once.S member, which
  references the Rust member but not the other way round. A static throw linked the keys without `pthread_once`: SIGSEGV
  (rc 139). Not PIE-specific.
* So both libraries end with "`__pthread_key_create` linked => `pthread_once` linked", the reference by keeping keys out unless
  threads are used, Liella (one Rust member) by pulling once.S's member whenever the Rust member is pulled.

## Fix (crates/boundary/src/posix/s22_threads.rs, keys section)
`#[used] static LINK_PTHREAD_ONCE` holds `crate::ffi::liella_machine_once` (once.S): the Rust member gets
`U __liella_libc_liella_machine_once` (checked with nm on the built libc.a), so the archive member with `pthread_once` comes
along. No new unsafe (a safe extern fn's address). `--gc-sections` drops the word in a static link.
Rejected: `global_asm!` `.reloc R_X86_64_NONE` (zero bytes, but `global_asm!` is `unsafe_code` and no UNSAFE category
covers it: a policy ruling, not worth it here).

## Tests (crates/core/tests/cxx_throw_products.rs; g++ absent => `SKIPPED:` line)
`static_cxx_throw_without_threads_runs`, `static_cxx_throw_with_one_thread_runs` (oracle/cxx_throw_plain.cc, -DWITH_THREAD),
`static_c_pthread_once_alone_runs` (oracle/once_alone.c), each with and without `--gc-sections`.
A symbol-table assertion was tried and dropped: plants showed it never fired (no unresolved `w` entry is listed for the
mutant), the run's signal 11 is what catches it.

## Jobs (kanon)
* gate3 `static-cxx-throw-pthread-once-gate3` rc 0 (clippy -D warnings, xtask static, affected --base origin/next: 6 suites ok, 612 passed, 0 failed; conformance 17/0) and plants3
  `static-cxx-throw-pthread-once-plants3` rc 0: 2 CAUGHT, 0 SURVIVED, 0 WARN.
* Superseded: gate (my test's first invariant too strict), plants (baseline red, same), gate2 (clippy indexing), plants2
  (both plants failed with signal 11 but not the msg regex: WARN-NO-MSG).

## INFORMATIONAL / RULING
* No decision id was reserved in the brief, so no D-row was added; recommend a one-line §22 row for the link edge.
* The kept word is also in libc.so.6 (`nm` shows LINK_PTHREAD_ONCE after `--gc-sections`): one 8-byte data word plus one
  RELATIVE relocation at load. NOT measured. If that matters, a follow-up can keep it out of the .so (e.g. a feature for
  the archive build).
* docs/codemap/symbols.tsv names s22_threads.rs:986 for `pthread_once`; it lives in csys/src/machine/once.S (stale row).
* No differences row: behaviour matches the reference (the throw runs).

## Next step
Read gate3/plants3 (`~/bin/jobwait static-cxx-throw-pthread-once-gate3 static-cxx-throw-pthread-once-plants3` on kanon).
