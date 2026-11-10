# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-2` (run 2, continuing `task/tb-foreign-safety`, from origin/next 21ef00b34).
Decision in force: **D-X.58** (records/decisions/D-X/D-X.58.toml; rulings #4212 registry now, #4213 the 0x80-0x9b
words are TB-FOREIGN-WORDS-LOW, #4214 the period cache 0x140 stays with the WCTYPE-TRIE-FILE chain).
Results of this run: see "Run 2" below and handoff/reports/TB-FOREIGN-SAFETY-2.json.

## The design (D-X.58, short)
Ownership = the process-level byte `liella_machine_owned` (machine/points.S), set by `init_initial` when the runtime
started the process; Rust (`thread::owned()`) and the asm entries read, write, pread, pwrite and close test it.
Elsewhere `thread::words()` gives the thread its words from a registry (64-entry mmap chunks keyed by thread pointer,
holding the kernel TID; a thread `create` made keeps its own block from `claim_own` to `release_own`; ENOMEM: one
shared static spare). Per word: 0x9c flags never read; 0xa0 thread, 0xa8 locale, 0xb0-0xc0 ctype, 0xc8 lconv,
0x128 h_errno, 0x130 res from the registry; 0x138 view never read (`foreign_view`).

## Run 2
- tbfs-gate-1 (run 1, shiki) failed on ONE test, `s07_locale_symbols::tb_read_locale_matches_thread_block`: it
  expected `uselocale` in a std thread to write word 0xa8 of that thread's block, which is exactly what D-X.58
  stops (the block is another library's). Fixed the test, not the code: it now checks the choice via
  `uselocale(0)` and that the block word is unchanged.
- Found and fixed: a forked child's only thread met its registry entry under a new TID and CLEARED it (uselocale
  choice, h_errno, descriptor lost after any fork in a foreign process; the other library's `fork` runs no hook of
  Liella's). Rule now in `foreign_words`: TID == getpid() means fork's copy (a process's first thread can only meet an
  entry keyed by its pointer that way), so the words stay. Test: s07 `foreign_thread_words_kept_in_forked_child`.
- New tests: `crates/boundary/tests/tb_foreign_words.rs` (one test; decoys in 0x9c-0x17f, bit 0 of 0x9c set, the
  descriptor decoy shows a pending cancellation, pointers at a zero area; asks toupper, tolower, isalpha/isdigit,
  towupper, `__ctype_b_loc`/`__ctype_toupper_loc` addresses, uselocale(0), localeconv address, h_errno set/get and its
  address, pthread_self, read/write on a pipe; answers equal the undecoyed ones, block words and decoy area unchanged,
  no allocation while decoys are in place). `conformance/c/tb_period_cache.c` (#4201: the period words at
  %fs:0x140-0x157 across a thread and a fork, TZ=EST5EDT,M3.2.0,M11.1.0).
- Plants: handoff/TB-FOREIGN-SAFETY-plants.txt (7: words() trusting FLAG_OWN, ctype_word, h_errno_location and view()
  ungated, the read entry's asm gate removed, the fork rule removed, the period word moved).
- Jobs on mei:~/w/s1: tbfs2-lint-0 (inherited head: clippy+static clean), tbfs2-check-1 (compile error in the new
  test, fixed), tbfs2-check-2 (static: opaque-audit, fixed with `#![no_builtins]`), tbfs2-check-3 (targeted tests and
  the conformance program green), tbfs2-gate-1 (clippy, static, records check, direct tier), tbfs2-plants-1.

## Open
- Counts NOT DONE (budget): before (origin/next) and after on isalpha/toupper, `__ctype_b_loc`, uselocale(0),
  `__h_errno_location`, errno, read (points.S), localeconv, pthread_getspecific, isw*. Expect +2 instructions on each
  gated path in an owned process (one `cmpb` + branch); 0 on the TID fast paths. This is the next step.
- codemap: run 1's `ops handback-check` was red with "docs/codemap/symbols.tsv out of date" on the head merged with
  next (next has `cargo xtask codemap`; this branch predates it). If still red, it needs a merge of origin/next
  (not allowed by this brief) or the coordinator's regeneration at landing: RULING NEEDED.
- Known limits (in D-X.58's design, not defects): a dead foreign thread's resolver state is not freed when its entry is
  reused (bounded by entries); TID reuse at the same thread pointer is indistinguishable from the same thread.

## Do not redo
- No ownership test inside the block can be safe (D-X.58 options). The asm gate is per entry (+2); a label-free form costs +3.
