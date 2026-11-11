# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-6` (run 6; continues -5 at a34cdac3f, no merge this run; merge-base with
origin/next ca8aa835a). HEAD: the report commit on top of 6653a4fbf. Decision in force: **D-X.58**
(records/decisions/D-X/D-X.58.toml; rulings #4212, #4213, #4214, #4241, #4253, #4287, #4779).
**Do not land as it stands: ruling #4779's condition is not met** (counts below: +4 to +7 per one-access call).

## The design (D-X.58, short)
Ownership = process-level byte `liella_machine_owned` (csys/src/machine/points.S), set by `init_initial`
(crates/boundary/src/thread.rs) when the runtime started the process. Rust reads a crate-private mirror
`thread::OWNED_HERE` (`thread::owned()`). In a process another C library started, `thread::words()` /
`foreign_words()` give the thread's registry words (64-entry mmap chunks keyed by thread pointer + TID; `SPARE`
when the registry has no memory). 0x9c flags and 0x138 view are never read in a foreign process.

## Every word access gated
| Site | Word | Owned process | Foreign process |
|---|---|---|---|
| points.S `read`/`write`/`pread`/`pwrite`/`close` entries | 0xa0 descriptor | `cmpb $0, liella_machine_owned(%rip)` then the fast path | the Rust path / slow path, no %fs read |
| locale.rs `view()` | 0x138 view, locale | `owned()` then the block words | `foreign_view()` (cold): registry `locale` |
| locale.rs `ctype_slot()` / `ctype_word()` | 0xb0-0xc0 | `owned() && CtypeB != 0` / `tb::read` | `foreign_ctype_slot` / unstored path with `foreign_word_or_zero` |
| locale.rs `uselocale`/`freelocale`/`lconv_char` | 0xa8, 0x128/0x130 | block words | registry words |
| netdb.rs h_errno, resolv.rs `res_state` | 0xc8, resolver word | block words | registry words (`SPARE` never freed) |
| ld.rs `__tls_get_addr` | %fs:8 dtv | `STARTED_HERE`, then dtv != 0, then the lookup | refuse, status 127 |

## Run 6
- Step 0 on the inherited head a34cdac3f: clippy + static green (shiki tb-foreign-safety-6-lint-0).
- **Why the two ld.rs plants survived in run 5:** the tests were right; the plants tool never rebuilt `ld.so` with
  the mutant. It rebuilds the products only for test targets the `affected` rule marks as products users (source
  names `target/liella`, `libc.a`, `liella-crt1` or `conformance`); the six liella-ld tests that run the built
  `ld.so` (tls, ldso_run, ldso_link, init_order, map, search_cache) write `target().join("liella")` and matched none,
  so both plants ran against an unmutated loader (their logs: "the baseline products, not rebuilt"). The same rule
  decides whether `cargo xtask affected` builds the products for them. Fix 247e8ce3b: `join("liella")` is a
  marker (xtask/src/affected.rs), a unit check names the six targets, docs/plants.md updated, plant
  `ld-products-marker-dropped`. Plants then: **10/10 CAUGHT**, both tls-get-addr plants included (plants-1).
- tbfs5-tests-1 (rc 1) was the dead hot-text keys before the regen (superseded by the regen). tbfs5-regen-2 (rc 1)
  was static private-paths on `tools/hottext/__pycache__/*.pyc` that regen.sh's python wrote into the role tree:
  regen.sh now sets `PYTHONDONTWRITEBYTECODE=1`.
- gate-1 (rc 1, 1 of 6424): `hot_text_products init_startup_path_lies_in_the_hot_text_range`:
  `liella_rt::start::program` in cold text. Run 5's `--allow-drop` regen dropped its startup key because no fast
  trace hit it, though `xtask so` had not called it dead. 6653a4fbf restores the line. Final head (6653a4fbf code): gate-2 green (11588 passed, 0 failed; conformance 128/0; peak 15G), plants-2 10/10 CAUGHT, percall-2 same counts as below.

## Ruling #4779 counts (instructions:u per call, wl_percall --spec validation/percall/locale.c, shiki, base = primed next ca8aa835a)
| case | glibc | next base | branch | delta |
|---|---|---|---|---|
| mb_cur_max | 15 | 12 | 18 | +6 |
| toupper_ascii | 24 | 24 | 28 | +4 |
| iswalpha_2byte | 51 | 29 | 35 | +6 |
| towupper_2byte | 41 | 28 | 35 | +7 |
| wcwidth_2byte | 45 | 50 | 57 | +7 |
| wcrtomb_2byte | 244 | 112 | 118 | +6 |
| mbrtowc_ascii / _2byte | 211 / 234 | 42 / 56 | 55 / 70 | +13 / +14 |
| regexec_short, mbstowcs_short | | 1248.5, 572 | 1262.6, 592 | +14, +20 (several accesses) |
**Above the accepted +2 (#4287).** Where it goes (wl_trace, `__ctype_get_mb_cur_max`, 11 instructions on the branch):
`push %rax; mov $abs,%rax; movzbl (%rax),%eax; test %al,%al; je <cold>; mov %fs:0x138,%rax; ...; pop %rcx; ret`.
The gate is 4 instructions, not 2: (1) `OWNED_HERE` is reached through a GOT slot (the static-link relaxation
leaves `mov $abs,%rax`), because the inlined `owned()` reads a static of another codegen unit in a PIC build;
(2) the atomic load is not folded into a `cmpb`. And (3) the cold `foreign_view()` call is not a tail call, so
the function gains a `push`/`pop` for stack alignment (+2). Run 3's mirror fix (82ad32ee1) did not remove (1).

## RULING NEEDED
1. #4779: measured +4..+7 per one-access call (table). Recommend: back to the story for codegen work: a direct
   rip-relative `cmpb $0, sym(%rip)` (a non-atomic byte written once before any thread, in the CGU of its readers
   or with hidden visibility; or a crate-wide relocation-model/visibility change, which needs its own ruling and
   would help every crate-private static), and a foreign path that does not force a frame on the owned path
   (tail-call shape). Target +2.

## Exact next step
The codegen fix above, with `wl_trace.sh --list` on
`__ctype_get_mb_cur_max` and toupper before/after, then wl_percall locale.c against the primed base.
DL-13's `__liella_tls_init`/`release` (not on next yet) need the same gate and foreign-block test (#4503, #4781).

## Open (D-X.58 known limits)
- A dead foreign thread's resolver state is not freed on entry reuse; TID reuse at the same thread pointer looks
  like the same thread.
- ld.so: in a process it started, every thread block is taken as its runtime's.

## Do not redo
- No ownership test inside the block can be safe (D-X.58 options). Plants: 10/10 CAUGHT (tb-foreign-safety-6-plants-1).
- The ld.rs tests were fine; the survivors were the products rule (above).
- Never `--allow-drop` a key `xtask so` does not call dead (start::program, run 5).
- wl_trace.sh refuses `~/wl/src/<sha>` as a tree (no .synced, no git): trace a base from a git tree.
