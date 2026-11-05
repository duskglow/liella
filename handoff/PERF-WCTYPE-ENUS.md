# PERF-WCTYPE-ENUS handoff (PERF-WCTYPE-ENUS-2: COMPLETE, target not reached)

State: branch task/perf-wctype-enus-2 (stacks on task/perf-wctype-enus, base origin/next 7e918cfe3). Role shiki:~/w/sh1.
Commits: 2815d0a2 (PERF-WCTYPE-ENUS: inline isw/wtrans hit paths, exhaustive test 0..=0x110000), e562595b
(-2: `#[inline(always)]` on `LocaleView::isw`/`wtrans`), then the saturation row and this handoff.

## What -2 found and fixed
-1's four jobs (keke) were all rc 0, but its counts showed two rows slower: `iswalpha_utf8` 68.0 -> 76.1 and
`towupper_utf8` 53.0 -> 68.0. The cause, from disassembling libc.so.6: with the inline hit paths, LLVM no longer
inlined `LocaleView::isw`/`wtrans` (plain `#[inline]`) into the exports. Every `iswalpha`/`towupper` (built-in locales
too) called them out of line. Fix: `#[inline(always)]` on both. The disassembled `iswalpha` saves no callee-saved
registers, and its C.UTF-8 arm is back to one call to `ctype::wclasses`.

## Results (shiki, Haswell, final code e562595b; register row: docs/saturation/15-*.md "PERF-WCTYPE-ENUS / -2")
* gate perf-wctype-enus-2-gate (xtask clippy, static, affected --base origin/next): rc 0, affected ok, peak 3.9G.
* plants perf-wctype-enus-2-plants (4 plants on the inline paths, reused from -1): 4 CAUGHT, 0 WARN.
* percall perf-wctype-enus-2-percall2 (wctype.c, en_US), base -> new (glibc): iswalpha_l_cjk 112 -> 82 (50),
  iswctype_cjk 96 -> 77 (46), towlower_cjk 102 -> 58 (41), iswalpha_ascii 61 -> 36.1 (22), towupper_l_latin1 80.2 -> 50.2
  (42). The other rows are at or below glibc.
* counts perf-wctype-enus-2-counts2 (cbench_locale before/after): iswalpha_en_us 91.1 -> 61.0 (49.1), iswalpha_l_en_us
  100.1 -> 70.0 (47.0), iswctype_en_us 95.1 -> 76.0 (43.0), towupper_l_en_us 68.0 -> 39.0 (39.0), iswalpha_utf8 68 -> 66,
  towupper_utf8 53 -> 53. newlocale_free +0.5 and fnmatch_casefold_en_32 +0.3 are within the run-to-run spread:
  in the same job, unchanged glibc moved -3.7 and -0.6 on those rows.
* After the docs commit: foreground `cargo clippy --workspace --all-targets --all-features -D warnings` rc 0, `cargo xtask static` rc 0.

## Not to redo
Micro-changes to the Table3 accessors (PERF-WCTYPE-L-2, NEGATIVE). Out-of-line `isw_wide` for the hit path (-1 profile).

## Next step (target 1.05x not reached for the CJK class rows: 61/49, 70/47, 76/43)
Profile `iswctype_en_us` first. It is 76 vs iswalpha_en_us 61, so the `wctype_t` mask decode probably sends it out of
line: check whether `iswctype` reaches `isw` with a non-constant mask (is_power_of_two test plus trailing_zeros), or
goes through `isw_wide`. Then the walk itself (about 35 instructions, three dependent bounds-checked loads):
PERF-WCTYPE-L-2's untried (a) lazy per-view BMP bit cache and (c) Table3 offsets resolved once at Wide construction.
(a) costs memory per locale object: ask for a maintainer ruling if a cache would be big.
Plan rows: none new (local detail). Read docs/saturation/15-*.md PERF-WCTYPE-L-2 and PERF-WCTYPE-ENUS / -2.

## Process notes
* wl_percall needs the base primed on the host (`bash validation/wl_prime_base.sh <host> <sha>` from a local checkout).
  The first percall/counts run (labels perf-wctype-enus-2-percall/-counts, rc 2) stopped on that.
