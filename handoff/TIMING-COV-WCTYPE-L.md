# TIMING-COV-WCTYPE-L handoff (INCOMPLETE: code done; jobs running; counts verdict and timing not done)

State: branch task/timing-cov-wctype-l (base origin/next e0c6b146), role keke:~/w/n2.
Commits: 0dcbace1 (Table3 word reads, Wide `Table3::EMPTY`, `LocaleView::isw_int` / `iswctype_view` -> int
tail calls, D-NEW row in plan/07-decisions/s05, plants file), 25dd7198 (MSRV 1.85: no `as_chunks`), then this handoff.

## Profile (base, build-host disassembly of Liella's libc.so.6, CJK U+4Exx in en_US)
* `__iswalpha_l` hit path about 58 instructions in the function: about 9 resolve the `locale_t` (null/alignment test,
  first-word load), 2 are a push/pop kept on every path for the cold calls, about 37 are the three-level walk
  (`bound` test plus a checked byte-range read with overflow tests per level, two absent-block tests, an `Option` test).
* `__iswctype` about 15 more than `iswalpha`: `push rbx` plus moves (the built-in arm needed the mask after its call),
  `desc <= 0xffff`, a 5-instruction power-of-two test, a stride-40 table index.
* The hypothesis in the brief (an out-of-line `wclasses` on these paths) was wrong for en_US: `wclasses` is only the
  built-in locales' arm (C.UTF-8: the iswalpha_utf8 row).

## Change (D-NEW, s05; the brief reserved no id: the coordinator assigns it)
1. Table3: level-1 entries as their own slice (one test covers `i1 < bound` and the read); each level is one
   checked word read at `offset / 4`. A misaligned offset (not well-formed, D.1.2) reads the word it falls in;
   `runs_equal` rounds the same way so it agrees with `value_i32`.
2. `Wide::class` holds `Table3::EMPTY` instead of `None` (one test less, same answers).
3. `LocaleView::isw_int` (`isw` = `isw_int != 0`), `isw_wide` and `iswctype_view` return the C int, so the cold arms
   are tail calls; `iswctype_view` sends built-in views to an out-of-line fn. All 36 `isw*` exports and the 4 iswctype
   forms use them. Before-change disassembly (intermediate head): `__iswalpha_l` lost its push/pop and about 8
   instructions; final: `__iswctype` saves no register (34 lines, no push).

## Jobs (keke) on head 25dd7198
* tcwl-gate2: clippy rc 0, static rc 0, `xtask so` and disassembly ok; then the direct tier
  (`affected --run --base origin/next`) and `bench --compare --counters cbench_locale`
  -> ~/work/TIMING-COV-WCTYPE-L/counts-head.txt. Still running at hand-back.
* tcwl-plants2: 5 plants (handoff/TIMING-COV-WCTYPE-L-plants.txt: wrong bit position, wrong level-2 word, wrong class bit
  in `isw_int`, wrong iswctype mask, `__iswalpha_l` with the current locale instead of its argument). All use
  `locale_products::locale_behaviour_equals_the_reference`; whether that test reaches `iswctype`/`__iswalpha_l` was NOT
  checked: a SURVIVED plant there needs a direct test (s05 clause), not a different plant.
* Superseded: tcwl-gate (rc 1: clippy incompatible-msrv on `as_chunks`), tcwl-plants (stopped, old head).

## Not done / next step
1. Read tcwl-gate2 and tcwl-plants2; fix any failure (run only the failing suite).
2. Counts: counts-head.txt has head vs glibc only. Base per-call figures are the brief's (other host): iswctype 76 vs 43,
   iswalpha_l 70.1 vs 47.0, iswalpha_en_us 61, iswalpha_utf8 66 vs 49, towupper_l 39 vs 39, nl_langinfo_l 35 vs 28.
   Run base on keke too (prime base, validation/wl_prime_base.sh) and check that no cbench_locale row got slower.
   Watch the collation rows: `Table3` also serves TABLEWC/COLLSEQWC, and the width and map tables.
3. Timing (brief): Ivy Bridge (nico or kotori, less busy) and N100, base vs head vs glibc, >= 3 rounds.
4. Untouched leftovers: `view_of` (about 9 instructions, null/alignment); iswctype's desc decode (u16 test plus
   power-of-two test); the iswalpha_utf8 arm (`wclasses` computes all classes); nl_langinfo_l / towupper_l IVB time
   (entry/layout, per the brief: after the main change).
Plan rows: D-NEW (s05, last row), D-7.13, PERF-WCTYPE-ENUS handoff.
