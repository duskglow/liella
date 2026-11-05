# PERF-REGRESS-1023-1105: rows whose instruction counts grew since 10-23

State: branch task/perf-regress-1023-1105 (from origin/next 87bc22cb). Code commit 07ce113b (gettext back-out).
Role mei:~/w/s2. Scratch: mei ~/work/perf-regress-1023-1105/ (before-*.txt, trace-head-iswalpha.txt, plants.txt, gate.sh).

## Step 1: the 8 worse rows (V-4 nico counters.tsv vs bench/parity-measure.tsv, instructions:u per call)

| row | 10-23 | a6fc34c2 (nico) | delta | glibc | 87bc22cb (mei, before) |
|---|---|---|---|---|---|
| isalpha_c | 22.0 | 23.0 | +4.5% | 16.0 | 23.0 |
| iswalpha_en_us | 74.1 | 100.1 | +35.1% | 49.1 | 100.1 |
| iswalpha_l_en_us | 83.1 | 109.1 | +31.3% | 47.1 | 109.2 |
| iswctype_en_us | 99.1 | 104.1 | +5.0% | 43.0 | 104.2 |
| gettext_language_alternating | 1114.6 | 1239.5 | +11.2% | 963.3 | 128x (cut) |
| gettext_language_steady | 854.1 | 1004.2 | +17.6% | 461.8 | 1004.0 |
| re_mt4_distinct | 216466.5 | 225563.8 | +4.2% | 145022.8 | - |
| sprintf_chk_ls_nlink | 820.6 | 882.9 | +7.6% | 734.9 | 877.7 |

kotori agrees on every row and adds re_mt4_shared (+3.9%); re_mt4_* differ between the two hosts (+4.2 / +6.7%) and are
4-thread rows (count depends on scheduling): not attributed, treat as noise until a repeat shows otherwise.
Note: the 10-24 "public tree" root (7374bf3f) is the oldest commit in this repo, so the 10-23 record's commit itself cannot be rebuilt.

## Step 2/3 per family

**gettext_language_* (FIXED in 07ce113b, after-count pending):** cause = landing 51d76b5a (GETTEXT-LANGUAGE-ALIGNED-COMPARE-2):
Host::language_probe/text_equals + strz::text_equals_words + Resolution::list_text, a fused word compare of LANGUAGE against
the last slot's list. Its own paired count (handoff/gettext-language-aligned-compare.md, run 2) measured 731.6 -> 862.9 (+131)
and said "do not land", but it landed. Speed attempt only (no correctness/security purpose), so it is backed out (code + its
test file); nothing it meant to deliver is lost. Trace: not taken (the cbench fixture catalog is not a wl_percall case); the
author's paired A/B is the evidence. crates/libc-so/hot-text.list keeps a stale `language_probe` key: generated file, merge.py
keeps old keys by design, harmless; the next regen drops it.

**iswalpha*/iswctype (NOT FIXED, profiled):** head trace (wl_trace, case iswalpha_cjk = U+4E00.., en_US.UTF-8, call 11):
91 instructions = is_w_file 62 + LocaleView::isw_wide 16 + iswalpha entry 13 (sequence entry:10 > isw_wide:16 > is_w_file:62 > entry:3).
Candidate cause: 860e50e1 (PERF-WCTYPE-UTF8-REST-3, 10-24) moved the file-backed arm of isw/wtrans/width out of line
(#[inline(never)] isw_wide) for the built-in locales' benefit (towupper_utf8 83 -> 53; iswalpha_utf8 81.1 -> 68.1 here).
The file-backed locale (en_US) now pays a call into isw_wide (16) plus a call into is_w_file. Same +26 on iswalpha and
iswalpha_l is consistent with one shared cost below the view lookup. NOT VERIFIED: no pre-860e50e1 trace (priming the old
revs timed out locally; budget). iswctype (+5) and isalpha_c (+1, likely 94ea6a7d's byte_class range test) are small.

**sprintf_chk_ls_nlink (+7.6%) and re_mt4: not investigated** (budget). Candidates: c3424c99 perf-sprintf-chk-nlink-5 landing.

## Next step
1. Prime 4b55627c and 860e50e1 on mei (validation/wl_prime_base.sh mei <rev>; run detached, it takes > 2 min) and
   `wl_trace.sh --spec ~/work/perf-regress-1023-1105/wct.c ~/wl/src/<sha> iswalpha_cjk iswalpha` at both, to confirm.
2. If confirmed: keep the built-in arm call-free but let isw_wide inline is_w_file (or mark is_w_file #[inline] into
   isw_wide only), and count iswalpha_utf8/towupper_utf8 too (they must not regress).
3. sprintf_chk_ls_nlink: per-call trace before/after c3424c99.
