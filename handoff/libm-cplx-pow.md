# LIBM-CPLX-POW handoff: cpow/cpowf quick + middle tiers, bounds proved, record pending (run 4)

State: branch task/libm-cplx-pow-4 (= -3's branch plus a merge of origin/next a5636ce6b, merge commit 1936e6f54);
head in handoff/reports/LIBM-CPLX-POW-4.json. Role mei:~/w/s4. Decisions in force: D-10.53, D-10.74, D-10.99,
D-10.25, D-10.17; this tier's record is pending (records/pending/LIBM-CPLX-POW-4-1.toml, D-NEW in series D-10:
the coordinator assigns the id).

## Blocker (still external): `cargo xtask so` fails on this branch, not on next

* origin/next a5636ce6b alone: `xtask so` rc 0, all 12 layout contracts hold (mei job `libm-cplx-pow-4-so-next`).
* This branch (next merged, 1936e6f54): clippy and static pass, then `so` stops with `hot-text.list: 1 key(s)
  match no function section of the fast-profile libc.a: _RINvNtC6liella6getopt4next<options::CArgs,
  options::CLongs>` (traced in tool) (job `libm-cplx-pow-4-lint0`). The locale::builtin key that blocked -3 is fixed
  (LAYOUT-HOTKEY-INLINE-STABLE); this is the next unpinned key of the same family.
* The branch changes only crates/math (+503 lines: the tiers, proofs and tests) and one xtask long-sweeps row. The
  key is a generic instantiated in liella_boundary; crates/libc-so/hot-text.unpinned:322 says it cannot be pinned
  (`#[inline(never)]` on a generic binds every instantiation). Growth of the cpow code reached through boundary
  flips its inlining. Not chased (brief: no regen, no dropped key; START-UP LAYOUT JITTER rule).
* Consequence: plants (the tool builds the products in its baseline), the `affected` gate and percall cannot run.

## Done in run 4

* Merged origin/next (the brief allows it); no conflicts.
* Decision record (pending): method, proofs, strict test, measured decline rates, citations (Joldes-Muller-Popescu
  2017; Muller, Elementary Functions 2016). `records check` ok.
* Plants file handoff/LIBM-CPLX-POW-4-plants.txt: -3's five (strict test dropped; LOG_DD_REL, ATAN_DD_REL, LR_ABS,
  LI_REL halved) plus `middle-bound-loose` (middle part bound times 1e15: still correct, declines, only
  `cpow_decline_rates` catches it; release, `--ignored`). NOT RUN (blocker).
* Job `libm-cplx-pow-4-check3` rc 0 on the code of the final head: records check, `cargo test -p liella-math --lib
  complex_fast::pow` (3 pass, the sweep ignored as a long sweep), `cargo xtask static`.

## Next step (continuation, once this branch's `so` passes)

1. Sync; `cargo xtask plants handoff/LIBM-CPLX-POW-4-plants.txt` and the `affected` gate (base origin/next).
2. percall typical and per-|w log z|-band hard rows (with and without `--tunables glibc.cpu.hwcaps=-FMA`) vs next
   and glibc; report the middle tier's cost per band (the quick tier declines 2^-2.4 per part in the large band);
   add the numbers to the pending record's body.
3. In-place entry (optional, after the above): `cplx2_fast` in crates/boundary/src/mathret.rs mirroring
   `cplx_fast`, rows `cpow f`/`d` in crates/boundary/src/posix/s10_math.tsv, `cargo xtask math-gen`. Its fast
   function must raise only inexact in hardware where it answers (prove no intermediate underflow/overflow on the
   answered path, or decline first); more boundary code, so check `so` again.

Do not redo: the proofs (log_dd 29.8u², atan_dd 34.6u²), the constants, the proof test, the decline sweep and its
long-sweeps row, the record text.
