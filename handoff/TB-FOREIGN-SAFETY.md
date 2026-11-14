# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-13` (run 13 = finish-only rerun of the percall job; code as run 12, which continues -11's head ac9a4d95a, which merged origin/next d53c0d551
with trace-at-build the default). Code commits 15e4256d4 + 8cb30e97a (then the handoff/report commit). Decision in force:
**D-X.58** (records/decisions/D-X/D-X.58.toml; rulings #4212, #4213, #4214, #4241, #4253, #4287, #4779, #4828, #4888,
#4889, #5344). **The gate now costs the one-access locale rows nothing: per call they equal next's counts (table).**

## The design (D-X.58, short)
Ownership = process-level byte `liella_machine_owned` (csys/src/machine/points.S, `.hidden`), set by `init_initial`
(crates/boundary/src/thread.rs) when the runtime started the process; the asm cancellation points test that byte.
Rust's mirror `thread::OWNED_HERE` is now a **word: all ones when owned, else 0** (run 12). `owned_mask()` reads it
with a plain read (`*OWNED_HERE.as_ptr()`, unsafe category *process-flag*, ruling #4888; static's body check unchanged);
`owned()` is `owned_mask() != 0`. A hot path that loads a thread-block word and tests it for zero reads
`tb::read::<F>() & owned_mask()`: the `and`/`test` with a memory operand takes the `test`'s place, so the gate adds no
instruction; in a foreign process the result is 0 whatever the block holds and the call goes to its unstored path.
In a process another C library started, `thread::words()` / `foreign_words()` give the thread's registry words.

## Run 12 (CI red: cbench_hot/ctype_get_mb_cur_max 12.0 over its limit 12; next 10)
- Step 0 on ac9a4d95a: mei tb-foreign-safety-12-lint-0 (workspace clippy --all-features, xtask clippy, static): rc 0.
- Cause: the D-X.58 `cmpb $0, OWNED_HERE(%rip)` + branch on every one-access locale path (+2).
- 15e4256d4: the mask word; `view` word (owned_view) and ctype words (`ctype_word`, `ctype_slot`) read ANDed with it.
  Measured: toupper back to next, but MB_CUR_MAX still +2: with `owned()` tested inline on the unstored branch the
  compiler kept the mask in a register for it (a separate `mov` load + a register move instead of the memory operand).
- 8cb30e97a: `view`, `with_view`, `isw_int`, `wtrans` send a zero `view_word()` to one cold function each
  (`unstored_view_cold`, `unstored_with_view`, `unstored_isw_int`, `unstored_wtrans`) that tests `owned()` there:
  owned and unstored -> `view_of(Locale word)`, foreign -> registry words. Disassembly (Release libc.a, mei):
  `__ctype_get_mb_cur_max` = `mov fs:0x138; and rax,[rip+OWNED_HERE]; je <tail jump unstored_with_view>; movzx; ret`
  (next: same with `test rax,rax`); `__ctype_b_loc` = `mov fs:0xb0; test [rip+OWNED_HERE],rax; je; mov fs:0; add; ret`.
- Wording: policy.rs, docs/architecture.md (process-flag row) and D-X.58 say "word"; D-X.58's cost paragraph says how
  the fold works and that in a foreign block the word is loaded and its value discarded (never a pointer or decision).
- lint-2 (clippy + static on 8cb30e97a): rc 0. Plants (handoff/TB-FOREIGN-SAFETY-12-plants.txt, 5 plants on the
  decoy test tb_foreign_words): plants-1 on 8cb30e97a: 4 CAUGHT, 1 SURVIVED (`view-unstored-ungated`: reading the
  foreign block's `locale` word went unseen because the decoy resolves to "C", the test's own global locale).
- 10301e40c: tb_foreign_words sets the global locale to C.UTF-8 first (MB_CUR_MAX 6), restores "C" at the end; a
  view resolved from the block's word now answers 1. gate-1 (FULL, on 8cb30e97a) stopped as stale (kill-label).
- Final jobs on 10301e40c (mei): tb-foreign-safety-12-lint-3 (workspace clippy --all-features, xtask clippy, static)
  rc 0; tb-foreign-safety-12-plants-2 rc 0: 5 caught (`view-unstored-ungated` now caught), 1 WARN-NO-MSG
  (`mask-always-owned` ended the test process with SIGABRT, a signal the plant's msg regex does not list; a later
  plants file should add `SIGABRT|signal: 6` to it); tb-foreign-safety-12-gate-2 (`affected --run --base
  origin/next`, escalates to FULL as every run of this story), pending at hand-back.

## Run 13 (finish only, no code change; branch `task/tb-foreign-safety-13`, code = 10301e40c, head 94a1c46a1 + this note)
- Why -12 went red: its percall-2 ran on 8cb30e97a, before 10301e40c changed crates/boundary/tests/tb_foreign_words.rs.
- mei tb-foreign-safety-12-gate-2 (on 10301e40c) ended rc 0, and so did plants-2 and lint-3.
- keke tb-foreign-safety-13-lint on 94a1c46a1 (workspace clippy --all-features -D warnings, xtask clippy, static): rc 0.
- keke tb-foreign-safety-13-percall on 94a1c46a1 (Zen 1), same 8 cases:
  `bash validation/wl_percall.sh --spec validation/percall/locale.c --cases mb_cur_max,toupper_ascii,iswalpha_2byte,towupper_2byte,wcwidth_2byte,wcrtomb_2byte,mbrtowc_ascii,mbstowcs_short .`
  rc 0. It gave the same numbers as the table below, case for case (glibc mbstowcs_short 452.1 there).

## Counts (instructions:u per call, validation/percall/locale.c, Release, mei Skylake-SP, base = next d53c0d551)
mei tb-foreign-safety-12-percall-2 on 8cb30e97a (percall on 15e4256d4 in brackets):
| case | glibc | next | run 12 | delta |
|---|---|---|---|---|
| mb_cur_max | 15 | 12 | 12 [14] | **0** |
| toupper_ascii | 24 | 24 | 24 [24] | 0 |
| iswalpha_2byte | 51 | 30 | 30 [32] | 0 |
| towupper_2byte | 41 | 29 | 29 [31] | 0 |
| wcwidth_2byte | 45 | 48 | 48 [50] | 0 |
| wcrtomb_2byte | 244 | 91 | 95 [97] | +4 |
| mbrtowc_ascii | 211 | 42 | 51 [53] | +9 |
| mbstowcs_short | 451 | 562 | 570 [572] | +8 |
(cbench_hot measures the same entries with less loop overhead: next 10 there, limit 12.)

## Exact next step
Land (all named jobs green: mei gate-2, plants-2, lint-3; keke -13 lint and percall). If CI is red, fix only a failure
this branch causes (that suite alone); a start-up bound red on unchanged start-up code is `INCOMPLETE: LAYOUT-JITTER`
with numbers, not a chase. Plants file note: add `SIGABRT|signal: 6` to `mask-always-owned`'s msg regex next time.
Follow-up (own story): mbrtowc/mbstowcs/wcrtomb are still +4..+9 over next. Their quick paths call
`locale::current()` mid-function; `view()`'s unstored/foreign case is now a call (next's was call-free inline), so
those entries pay frame/spill cost. Fix: shape `quick` so the view is reached through a tail-call helper (as
`with_view`), or pass the encoding through one cold function; measure with the percall command above.

## Open (D-X.58 known limits)
- A dead foreign thread's resolver state is not freed on entry reuse; TID reuse at the same thread pointer looks
  like the same thread.
- ld.so: in a process it started, every thread block is taken as its runtime's.
- next's `tls_visit` reads the listed block's `FLAG_OWN`, reachable only through this loader's `StartInfo` (run 11).

## Do not redo
- No ownership test inside the block can be safe (D-X.58 options). An atomic load never folds into a memory operand.
- Do not test `owned()` inline next to a masked read: the compiler shares the mask in a register (+2). Keep the
  owned() test in the cold function the zero case jumps to.
- One `with_view` closure shared by many entries is outlined; use a cold function per helper (`isw_int`, `wtrans`).
- A plant that plants code into another file must compile there, or `cargo xtask static` stops at the build.
- Never `--allow-drop` a key `xtask so` does not call dead (start::program, run 5).
- Runs 8-11 detail: git history of this file on task/tb-foreign-safety-11.
