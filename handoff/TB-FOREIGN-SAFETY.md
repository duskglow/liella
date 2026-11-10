# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety` (from origin/next 21ef00b34), INCOMPLETE (run 1 stopped at the budget
checkpoint). Code is written and compiles: clippy is clean (shiki tbfs-clippy-2) and static found nothing
(tbfs-static-1). The direct-tier gate tbfs-gate-1 was started on shiki:~/w/sh2 and had not been read when the run
ended. Not done: the foreign-block test, the period-cache conformance test, plants, counts, and the D-X.58 record.

## Decision (to file as D-X.58; `cargo xtask records new decision D-X.58` on a host, no local cargo)

Ownership test = **the process-level fact `owned()`**: the runtime started the process. The flag is now a hidden
byte `liella_machine_owned` (machine/points.S), so Rust and the asm entries read the same byte. When the runtime
starts the process, every thread that runs library code has a block the runtime made, because Liella creates every
thread. The test never reads a word in the block. FLAG_OWN (bit 0 of 0x9c) is still written into Liella-made blocks
but is no longer read anywhere.

Where the test fails (a test process, or a program under another loader, D-22.10), `thread::words()` returns the
thread's words from a registry. The registry is chunks of 64 entries (each an anonymous mapping, never unmapped),
keyed by thread pointer with linear probing, and each entry holds {tid, own, a block-shaped area}. A different kernel
TID at the same thread pointer means a new thread on a reused block, so its words are cleared. A thread Liella
created marks its entry `own` (`claim_own` at the start of `entry`, `release_own` at the end of `finish`) and keeps
using its own block, so join, cancellation and setlocale's `each_block` work as before. If no chunk can be mapped
(ENOMEM), the thread gets one shared static spare (all atomics, memory-safe; per-thread state becomes process-wide).
`resolv::thread_ending` never frees the spare's resolver state.

Options considered (hot-path cost, before counts):
1. Process fact `owned()`: one `cmpb $0, sym(%rip)` plus a branch, about +2 instructions on gated paths. In the TID
   fast paths it replaces the 0x9c load and test (+0). Safe by construction. **Chosen.**
2. A value only Liella's constructor writes (e.g. tp XOR a secret at 0x9c): a load, an xor and a compare (+3). A
   foreign block passing is negligible but not impossible, and a program can plant the value. Rejected.
3. Look the address up in Liella's registry on every access: a lock or list walk per call. Rejected for hot paths.
   It is what the foreign-only slow path does (the registry plus `gettid`).
4. ELF TLS (`__thread` in csys) for the foreign words: exact per-thread semantics, but it adds PT_TLS to
   libc.so/libc.a and needs loader support for libc's own TLS. Not tried. Worth a ruling if the registry's `gettid`
   per foreign call turns out too slow.

## Each word in a foreign block (0x9c-0x137, plus 0x138 read by `view()`)

None of 0x9c-0x137 is shared ABI. Programs reach errno and h_errno only through `__errno_location` and
`__h_errno_location`; no ABI fixes their offset. The only TCB words fixed by ABI are 0x0, 0x10, 0x28 (stack guard)
and 0x30 (pointer guard).

| word | owned process | foreign block (after this change) |
|---|---|---|
| 0x9c flags | written by the block constructors | never read (`fast_tid`, `own_tid_fast`, `own_tid` test `owned()`) |
| 0xa0 thread | `%fs` load (Rust, points.S) | registry `thread` (`current`, `adopt`); points.S skips to the Rust path |
| 0xa8 locale | `%fs` load | registry (`uselocale`, `foreign_view`) |
| 0xb0-0xc0 ctype | `%fs` loads | registry (`ctype_word`, `foreign_ctype_slot` filled on first use) |
| 0xc8 lconv | block | registry (`localeconv`) |
| 0x128 h_errno | tp+0x128 | registry address (`h_errno_location`, get, set) |
| 0x130 res | block | registry (`res_state`, `thread_ending`, `print_slot`) |
| 0x138 view | `%fs` load | never read (`view()` returns early with `foreign_view`); never written (already gated) |

## Open, and must be done before landing
- `ops handback-check` at 67714a9e0 was red at the lint step with "codemap FAILED: docs/codemap/symbols.tsv out of
  date: run cargo xtask codemap". This checkout's xtask usage lists no `codemap` subcommand, and the run ran out of
  budget before finding the generator. Regenerate the codemap with whatever produces it (it is a generated file:
  never hand-edit it) and copy the result back to the worktree.
- Tests (spec trace `no spec clause`, D-X.58). (a) A dedicated test file with a single test, so its process has
  only the harness thread and this one. In a std thread: save words 0x9c-0x187, fill them with decoy values (bit 0
  of 0x9c set; pointers aimed at a static decoy area so a planted mutant gives a message, not SIGSEGV), and call
  only Liella (byte_class, toupper, `ctype_slot`, `uselocale(0)`, `set_h_errno`/`h_errno`, `h_errno_location`,
  `localeconv`, `thread::current`). Record the results and whether the words are unchanged, restore the words, then
  assert. Never call glibc while the decoys are in place. (b) `conformance/c/tb_period_cache.c` (#4201): port
  `tz_period_words_per_thread_and_fork` (crates/boundary/tests/s23_time_symbols.rs:539) to C, reading
  `%fs:0x140..0x150` by inline asm; RUN-ENV TZ=EST5EDT,M3.2.0,M11.1.0; pthread_create and fork.
- Plants (`cargo xtask plants`): words() trusts FLAG_OWN (`owned() || tb::read32::<tb::Flags>() & FLAG_OWN != 0`);
  h_errno_location ungated; ctype_word ungated; points.S gate removed (anchor to `liella_machine_read:`).
- Counts before (origin/next) and after: isalpha/toupper, __ctype_b_loc, uselocale(0), __h_errno_location, read
  (points.S), localeconv, pthread_getspecific. Expect about +2 instructions on each gated path.
- The gate tbfs-gate-1 may show in-process tests that assumed Liella's words live at tp+0xb0 etc. in a test
  process. Those assumptions no longer hold, so fix the tests (the registry is the right home).

## Interaction with WCTYPE-TRIE-FILE-3 (not on origin/next)
That branch gates 0x138-0x187 (view, cache line, wclass) on its own. This branch changes `view()` (an early
foreign return) and does NOT touch the period cache (zone.rs `with_period` still uses the block in test processes).
When both land, `owned()` / `words()` are the shared test; expect a merge in `locale::view`.

## Do not redo
- No ownership test inside the block can be safe; the reasoning is above.
- The asm gate is placed per entry (+2 instructions). A label-free form costs +3.
