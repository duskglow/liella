# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-4` (run 4; continues `task/tb-foreign-safety-3`, same code), origin/next
2e33a3cb9 merged (ruling #4253). HEAD: the handoff commit on top of the merge 9698c773b. **BLOCKED (RULING NEEDED,
below): `cargo xtask so` fails layout contract STARTUP-DATA-WINDOWS in both profiles (landed on next today, D-X.50)**,
so no gate, plants or counts can run on this head. clippy and static green on it (tbfs4-lint-2).
Decision in force: **D-X.58** (records/decisions/D-X/D-X.58.toml; rulings #4212, #4213, #4214, #4241, #4253, #4287).

## The design (D-X.58, short)
Ownership = process-level byte `liella_machine_owned` (csys/src/machine/points.S), set by `init_initial`
(crates/boundary/src/thread.rs) when the runtime started the process. Rust reads a crate-private mirror
`thread::OWNED_HERE` stored beside it (`thread::owned()`: one rip-relative `cmpb`). In a process another C library
started, `thread::words()` / `foreign_words()` give the thread's registry words (64-entry mmap chunks keyed by thread
pointer + TID; `SPARE` when the registry has no memory). Per word: 0x9c flags never read; 0x138 view never read.

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

## Run 4
- Step 0 on the inherited head 9cb75fa1f: clippy + static green (tbfs4-lint-1).
- Merged origin/next 2e33a3cb9 (8 commits: LAYOUT-STARTUP-DATA, D-X.50, new contract STARTUP-DATA-WINDOWS; no
  conflicts).
- `regen.sh . --build` (handoff step 1) fails before tracing, in both profiles, on STARTUP-DATA-WINDOWS:
  fast 4 read-only start-up windows > ro-budget-fast 3 (tbfs4-regen-1); release 5 > 4 (tbfs4-regen-rel-1).
  origin/next itself, same host: fast 2 windows / 20 pages, contract ok (tbfs4-nextso-1).
- Cause (probe links of `page_faults.c`, ~/work/TB-FOREIGN-SAFETY-4/{next,role}.{nm,sec} on mei): the branch's
  .rodata is +0x30 bytes, but its .text in the probe link is +0x6040 (fast), so .rodata starts 0x6000 higher and
  both start-up rodata clusters straddle a 64 KiB boundary (fast: 0x75e5f8-0x763ffd and 0x7d7450-0x7e3600).
  Of the text growth only ~7.5 KB is sized symbols and most of that is ThinLTO repartitioning churn in unrelated
  functions (argp `write_help` +10.6 KB, nss `*_cached` instances, mbrtoc* -7 KB). The release lone window holds the
  tunables type string `runtime::start` compares (also on next). No start-up data object comes from this story.
- The 4 dead fast hot-text keys of run 3 are still there (cannot regenerate: regen builds with the layout check).

## RULING NEEDED
1. STARTUP-DATA-WINDOWS read-only budgets depend on total text size modulo 64 KiB (next sits at 2 of 3 by layout,
   D-X.50 measured 3). Recommend: cut the D-X.50 item-3 follow-up for read-only data first (put the start-up
   functions' rodata input sections in one place, e.g. `.rodata1`, so the count stops moving with text size) and
   land this branch on top; alternative: accept +1 ro window per profile for this branch (a loosening: owner's call).
   Padding text to dodge a boundary was not done (it fixes nothing).
2. (standing, #4287) +2 instructions per locale-word access in owned processes: counts on the final head still owed.

## Exact next step (once the layout ruling is applied)
1. On the role: `bash tools/hottext/regen.sh . --build`, then `LIELLA_FULL_PROFILE=1 bash tools/hottext/regen.sh . --build`
   (fixes the 4 dead keys; Features::detect is a STARTUP key, live in release only). Commit the list files.
2. Tests tb_foreign_words + s07_locale_symbols; direct gate; plants (handoff/TB-FOREIGN-SAFETY-plants.txt);
   counts `bench --compare --counters --runs 3 cbench_hot cbench_locale` on that head. Base: next's product code is
   unchanged since tbfs3-counts-base-1 (the 8 new commits are layout tooling), copy at
   mei:~/work/TB-FOREIGN-SAFETY-4/base-measure.tsv, join with ~/work/TB-FOREIGN-SAFETY-4/join.py. Want +2.

## Open (D-X.58 known limits)
- A dead foreign thread's resolver state is not freed on entry reuse; TID reuse at the same thread pointer looks
  like the same thread.

## Do not redo
- No ownership test inside the block can be safe (D-X.58 options). Plants 7/7 settled (#4254, tbfs3-plants-2).
- run 3's +6 per access was the extern byte; fixed to the `OWNED_HERE` mirror (82ad32ee1), not yet counted.
- Jobs on mei, run 4: tbfs4-lint-1 (pre-merge head, green), tbfs4-regen-1 / tbfs4-regen-rel-1 (rc 1, the layout
  evidence), tbfs4-nextso-1 (next baseline), tbfs4-roledump-1 (rc 1, my script), tbfs4-roledump-2, tbfs4-lint-2.
