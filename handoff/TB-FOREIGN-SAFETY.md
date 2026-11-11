# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-5` (run 5; continues -4, same code plus the ld.so path). origin/next
ca8aa835a merged (LAYOUT-STARTUP-RODATA closed with its goal met on next, ruling #4297). HEAD: the report commit on
top of the hot-text regen. Decision in force: **D-X.58** (records/decisions/D-X/D-X.58.toml; rulings #4212, #4213,
#4214, #4241, #4253, #4287).

## The design (D-X.58, short)
Ownership = process-level byte `liella_machine_owned` (csys/src/machine/points.S), set by `init_initial`
(crates/boundary/src/thread.rs) when the runtime started the process. Rust reads a crate-private mirror
`thread::OWNED_HERE` stored beside it (`thread::owned()`: one rip-relative `cmpb`). In a process another C library
started, `thread::words()` / `foreign_words()` give the thread's registry words (64-entry mmap chunks keyed by thread
pointer + TID; `SPARE` when the registry has no memory). Per word: 0x9c flags never read (run 5: the `tb::Flags`
reader is removed, so no code can read it); 0x138 view never read.

## Every word access gated, and how (for the coordinator's review)
| Site | Word | Owned process | Foreign process |
|---|---|---|---|
| points.S `read`/`write`/`pread`/`pwrite` entries | 0xa0 descriptor | `cmpb $0, liella_machine_owned(%rip)` then the fast path | `je liella_<op>_rest` (Rust path, no %fs read) |
| points.S `close` entry | 0xa0 | same `cmpb` | `je 1f` (slow path) |
| locale.rs `view()` | 0x138 view, locale | `owned()` then the block words | `foreign_view()` (cold): registry `locale` |
| locale.rs `ctype_slot()` | 0xb0-0xc0 ctype words | `owned() && CtypeB != 0` -> `ctype_off` | one cold `ctype_slot_slow` -> `foreign_ctype_slot` (registry, refreshed under `serialized`) |
| locale.rs `ctype_word()` (class/case lookups) | 0xb0-0xc0 | `tb::read` | 0, so the existing unstored path runs; `class_unstored`/`case_unstored` read `foreign_word_or_zero` |
| locale.rs `uselocale`/`freelocale` | 0xa8 locale | `tb::read`/`tb::current` | `foreign_words().locale` / `thread::words()` |
| locale.rs `lconv_char` | 0x128/0x130 lconv | `thread::words().lconv` (block in owned) | registry `lconv` |
| netdb.rs `__h_errno_location`, get/set | 0xc8 h_errno | block word at tp+offset | registry `h_errno` (address stable per thread) |
| resolv.rs `res_state`, free, `print_slot` | resolver state word | `thread::words()` | registry word; `SPARE` (shared) is never freed |
| **ld.rs `__tls_get_addr` (run 5, coordinator #4503)** | %fs:8 dtv (psABI slot; 0x9c no longer read) | `STARTED_HERE` (ld.so's own byte, set by `install_tls`) then dtv != 0, then the lookup | refuse, status 127 ("called in a process this loader did not start") |

## Run 5
- Step 0 on the inherited head 1e28dff71: clippy + static green (kanon tbfs5-lint-0).
- Merged origin/next ca8aa835a: one conflict, xtask/src/coldbss.rs: next's `split_stem` covers the same ThinLTO
  split-static suffix the branch had fixed; next's file taken whole. docs/codemap/symbols.tsv reset to next's
  (generated view).
- ld.so `__tls_get_addr` brought under the process rule (069fdfd4f); zero-dtv refusal; tests in crates/ld/tests/tls.rs:
  `tls_get_addr_refuses_in_a_process_it_did_not_start` (host-libc program dlopens the built ld.so, forged block
  with bit 0 of 0x9c and a valid-looking dtv), `tls_get_addr_refuses_a_thread_without_a_vector`. D-X.58 body updated.
- STARTUP-DATA-WINDOWS holds on the merged head (fast: 1 ro window, budget 1). Hot-text regen (both profiles) with
  `--allow-drop` for 2 fast keys no trace hit (`liella_rt::start::program`, `iconv::convert`; neither touched by this
  story; `nm` counts in tbfs5-regen-2's log). tbfs5-regen-1 (rc 1) was the refusal without `--allow-drop`.
- Plants: handoff/TB-FOREIGN-SAFETY-plants.txt, 9 (7 settled in run 3 + 2 for ld.rs).

## RULING NEEDED
1. (standing, #4287) +2 instructions per locale-word access in owned processes: the final-head counts job
   (tbfs5-counts-1, `bench --compare --counters --runs 3 cbench_hot cbench_locale`) ran; the delta against a next
   base was not measured in this run (budget). The run-4 base (mei:~/work/TB-FOREIGN-SAFETY-4/base-measure.tsv) is
   stale: next has moved 422 commits.

## Exact next step
Read the verdicts of tbfs5-gate-1, tbfs5-plants-1, tbfs5-counts-1 (kanon). For #4287: prime a next base and compare
the locale rows (want +2). DL-13's `__liella_tls_init`/`release` (not on next yet) need the same process-level gate and
the same foreign-block test as `__tls_get_addr` (coordinator #4503).

## Open (D-X.58 known limits)
- A dead foreign thread's resolver state is not freed on entry reuse; TID reuse at the same thread pointer looks
  like the same thread.
- ld.so: in a process it started, every thread block is taken as its runtime's (a thread made with a raw `clone` and
  its own TLS is the program's responsibility, as everywhere under D-X.58).

## Do not redo
- No ownership test inside the block can be safe (D-X.58 options). Plants 7/7 settled (#4254, tbfs3-plants-2).
- run 3's +6 per access was the extern byte; fixed to the `OWNED_HERE` mirror (82ad32ee1).
- Jobs, run 5 (kanon): tbfs5-lint-0 (inherited head, green), tbfs5-tests-1 (rc 1: dead hot-text keys before regen),
  tbfs5-regen-1 (rc 1: regen refused the 2 drops), tbfs5-regen-2, tbfs5-gate-1, tbfs5-plants-1, tbfs5-counts-1.
