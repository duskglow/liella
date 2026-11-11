# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-7` (run 7; continues -6 at 70eeb1619, no merge; merge-base with origin/next
ca8aa835a). HEAD: the report commit on top of e8c366306 (code: 5f02254fb, then three test-only commits). Decision in
force: **D-X.58** (records/decisions/D-X/D-X.58.toml; rulings #4212, #4213, #4214, #4241, #4253, #4287, #4779, #4828).
**Ruling #4779's condition (<= +2 per one-access call) is still not met: +4 on every row in the counts (Fast-profile
products), down from +4..+7.** Do not land until RULING NEEDED 1 below is decided.

## The design (D-X.58, short)
Ownership = process-level byte `liella_machine_owned` (csys/src/machine/points.S, `.hidden`), set by `init_initial`
(crates/boundary/src/thread.rs) when the runtime started the process. Rust reads a crate-private mirror
`thread::OWNED_HERE` (`thread::owned()`). In a process another C library started, `thread::words()` /
`foreign_words()` give the thread's registry words. 0x9c flags and 0x138 view are never read in a foreign process.
Gated sites: see the table in -6's note (unchanged): points.S entries, locale.rs view/ctype words/uselocale/lconv,
netdb/resolv, ld.rs `__tls_get_addr`.

## Run 7 (ruling #4828: make the gate cheap)
- Step 0 on 70eeb1619: clippy + static green (mei tb-foreign-safety-7-lint-0).
- 5f02254fb: **tail-call shape.** `locale::with_view(f)` / `with_current(f)`: the foreign case hands the entry's
  last step (a closure, by value) to the cold generic `foreign_with_view`, a `jmp`, so the owned path keeps no frame.
  Used by `MB_CUR_MAX`, `wcwidth`, `btowc`, `wctob`, `towctrans`, `iswctype`. `isw_int` / `wtrans` (the twelve
  `isw*` and `tow*` entries) use a cold function each instead: one closure shared by twelve entries was called, not
  inlined (seen in the disassembly). Release disassembly: `__ctype_get_mb_cur_max`, `iswalpha`, `towupper` lost
  their `push`/`sub rsp` prologue. The foreign-block test now also asks `iswalpha`, `MB_CUR_MAX` and the address
  `view()` returns (`towupper` no longer reaches `view()`, and `mblen` of an ASCII byte answers the same through
  any view: plants-1 and -3 had `view-ungated` SURVIVED for that; fixed in e8c366306).
- Jobs (mei): lint-4 (clippy workspace + xtask clippy + static) green; gate-4, plants-4, percall-3 on e8c366306
  (results in the report json and the hand-back). Superseded: gate-1/-2/-3 (stopped: the head moved), plants-1/-3
  (view-ungated survived), lint-2 (clippy manual_c_str_literals), percall-1/-2 (same code, earlier heads).
- **Why the gate is still 3-4 instructions, not 2 (measured, mei):**
  1. No atomic byte load is folded into `cmpb`: of 846 reads of `OWNED_HERE`/`LOCK_ALONE` in the release libc.a,
     every one is `movzbl` + `test` (`LOCK_ALONE` is `Relaxed`). Relaxed vs Acquire changes nothing on x86.
  2. GOT: `wl_percall`/`wl_prep` build their products with plain `cargo xtask so` = the **Fast** profile (thin
     LTO, 16 CGUs). There `OWNED_HERE` is an exported symbol (reachable through `pub #[inline]` fns such as
     `locale::view`), so it is read through `R_X86_64_REX_GOTPCRELX` (+1: `mov GOT,%rax; movzbl (%rax)`). In the
     shipped Release build (fat LTO, `LIELLA_FULL_PROFILE=1 cargo xtask so`) it is internal: `movzbl X(%rip)`.
  So the percall rows show +4 (GOT load + movzbl + test + je); the shipped build should show +3 (from the
  disassembly; Release per-call counts were NOT measured: the percall cache is keyed by sha, not profile).

## Ruling #4779 counts (instructions:u per call, wl_percall --spec validation/percall/locale.c, mei, base = primed next ca8aa835a, Fast-profile products)
| case | glibc | next base | run 6 | run 7 | delta vs next |
|---|---|---|---|---|---|
| mb_cur_max | 15 | 12 | 18 | 16 | +4 |
| toupper_ascii | 24 | 24 | 28 | 28 | +4 |
| iswalpha_2byte | 51 | 29 | 35 | 33 | +4 |
| towupper_2byte | 41 | 28 | 35 | 32 | +4 |
| wcwidth_2byte | 45 | 50 | 57 | 54 | +4 |
| wcrtomb_2byte | 244 | 112 | 118 | 118 | +6 |
| mbrtowc_ascii / _2byte | 211 / 234 | 42 / 56 | 55 / 70 | 55 / 70 | +13 / +14 |
| regexec_short, mbstowcs_short | | 1248.6, 572 | 1262.6, 592 | 1262.5, 592 | +14, +20 |
(run 6's column was measured on shiki; run 7 on mei; base and glibc columns agree.)

## RULING NEEDED
1. **The `cmpb` fold needs a non-atomic read of the write-once byte.** A plain (or volatile) read of a static
   written once in `init_initial` before any thread exists folds to `cmpb $0, X(%rip)`; it needs `unsafe` in
   thread.rs, which no category in docs/architecture.md S2 permits. Recommend: a narrow category (e.g.
   *process-flag*: a non-atomic read of a byte written only before the first thread is created; thread.rs only),
   which gives +3 -> +2 in Release. Alternative: accept +3 (Release) for #4779.
2. **Per-call counts measure the Fast profile, not what ships** (`wl_prep` runs plain `cargo xtask so`). Here that
   costs +1 (a GOT load) that the shipped build does not have. Recommend: `wl_prep` builds with
   `LIELLA_FULL_PROFILE=1` (and keys its product cache by profile), since #4779-style instruction limits are judged
   from these counts. Also fixable here without tooling: make `owned()` reachable only crate-internally (no
   `pub #[inline]` caller), so the Fast build hides `OWNED_HERE` too; wide visibility change, not done.

## Exact next step
After ruling 1: read `OWNED_HERE` non-atomically in `owned()` (category per the ruling), check the release
disassembly for `cmpb $0, ...(%rip)`, then count (Release products if ruling 2 lands). mbrtowc (+13/+14) and
mbstowcs (+20) have several gated accesses per call: hoist one `owned()` per entry if the target extends to them.
DL-13's `__liella_tls_init`/`release` (not on next yet) need the same gate and foreign-block test (#4503, #4781).

## Open (D-X.58 known limits)
- A dead foreign thread's resolver state is not freed on entry reuse; TID reuse at the same thread pointer looks
  like the same thread.
- ld.so: in a process it started, every thread block is taken as its runtime's.

## Do not redo
- No ownership test inside the block can be safe (D-X.58 options). Run 6 plants: 10/10 CAUGHT.
- A Relaxed atomic load does not give `cmpb` (846 of 846 reads are movzbl/test). The GOT load is the Fast profile's.
- One `with_view` closure shared by many entries is outlined; use a cold function per helper (`isw_int`, `wtrans`).
- Never `--allow-drop` a key `xtask so` does not call dead (start::program, run 5).
