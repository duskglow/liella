# PERF-REGRESS-1023-1105: rows whose instruction counts grew since 10-23

State: branch task/perf-regress-1023-1105-2 (stacks on -1's 8c871919; from origin/next 87bc22cb). Code commits:
07ce113b (-1: gettext back-out), 5a3048c7 (-2: wide class walk inlined into the view's file arm). Plants file:
handoff/perf-regress-1023-1105-2.plants. Role kanon:~/w/kn1 (Broadwell E5-2699 v4); scripts kanon ~/work/perf-regress-1023-1105-2/.

## The 8 worse rows (V-4 vs bench/parity-measure.tsv, instructions:u per call) and where they stand

| row | 10-23 | 87bc22cb | -2 before (8c871919) | -2 after (5a3048c7) | glibc | state |
|---|---|---|---|---|---|---|
| isalpha_c | 22.0 | 23.0 | 23.0 | 23.0 | 16.0 | open, +1 (likely the byte_class range test; not traced) |
| iswalpha_en_us | 74.1 | 100.1 | 100.1 | **91.1** | 49.1 | partly fixed, +17 left |
| iswalpha_l_en_us | 83.1 | 109.2 | 109.1 | **100.1** | 47.0 | partly fixed, +17 left |
| iswctype_en_us | 99.1 | 104.2 | 104.1 | **95.1** | 43.0 | FIXED (below 10-23) |
| gettext_language_alternating | 1114.6 | 128x | 1101.9 | - | 1037.0 | FIXED by 07ce113b |
| gettext_language_steady | 854.1 | 1004.0 | 712.7 | - | 458.6 | FIXED by 07ce113b |
| re_mt4_distinct | 216466.5 | - | - | - | - | 4-thread row, host-dependent; not attributed |
| sprintf_chk_ls_nlink | 820.6 | 877.7 | 876.6 | 876.6 | 742.0 | open, see below |

-2 counts: kanon jobs perf-regress-1023-1105-2-before / -after (`cargo xtask bench --compare --counters --runs 5` cbench_locale,
cbench_opt with CBENCH_ONLY). Unchanged rows in the after run: towupper_utf8 53.0, iswalpha_utf8 68.0, towupper_l_en_us 68.0
(10-23 67.1, +1.3%, inside +-3%), gettext rows the same (no gettext code changed). No row slower.

## wctype: per-call split (wl_trace, case iswalpha_cjk, en_US.UTF-8, call 11; spec ~/work/perf-regress-1023-1105-2/wct.c)

Before (-1, mei): 91 = iswalpha entry 13 + LocaleView::isw_wide 16 + ctype::is_w_file 62. After 5a3048c7 (kanon job -trace): 82 =
entry 13 + isw_wide 69. Cause of the first 9: since 860e50e1 moved the file arm out of line, isw_wide had two call sites of
is_w_file (Latin miss and non-Latin), so it stayed a separate frame despite #[inline]. 5a3048c7 makes one call site and
#[inline(always)] (its only caller). iswctype (runtime mask, same path) is now 95.1, below its 10-23 99.1.

The +17 left on iswalpha/iswalpha_l is NOT the shared arm (iswctype with the same arm is below 10-23). At 10-23 iswalpha was
25 cheaper than iswctype; now 4. HYPOTHESIS (not verified): before 860e50e1 the arm inlined into each iswXXX entry with the
mask a constant (ALPHA), so the power-of-two test, trailing_zeros, the `bit & 15` bounds check and the Option<Table3> copy
folded away; the out-of-line isw_wide takes the mask at run time.

## sprintf_chk_ls_nlink (+56, not fixed)

Not caused by PERF-SPRINTF-CHK-NLINK (c3424c99): its own percall measured chk_ls 932.5 -> 907.0 at its base, so the row was
already high before it landed. The growth is between the 10-23 record and that story's base; no bisect done (budget).

## Next step
1. iswalpha*: trace isw_wide's 69 instructions against the hypothesis (constant mask). Candidate design: a const-generic
   `isw_class::<MASK>` out-of-line arm per standard class used by the iswXXX entries (one frame, mask folded), keeping
   `isw` (runtime mask) for iswctype; count iswalpha_utf8/towupper_utf8 too (must not regress).
2. sprintf_chk_ls_nlink: wl_percall chk_ls at the 10-23-era root and at the PERF-SPRINTF-CHK-NLINK base, then bisect.
3. isalpha_c +1: trace isalpha in the C locale.

## Not to redo
gettext back-out (done, counted); the is_w_file frame (done, 5a3048c7, 4/4 plants caught).
Note for docs/plants.md: its example uses `-p liella-core`, but the crates/core package is `liella` (my first plants run
failed its baseline on that).
