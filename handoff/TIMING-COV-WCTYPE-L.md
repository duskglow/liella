# TIMING-COV-WCTYPE-L handoff (run 2: TIMING-COV-WCTYPE-L-2)

State: branch task/timing-cov-wctype-l-2 (base origin/next e0c6b146), role shiki:~/w/sh2. HEAD: see the report json.
Commits on the branch: 0dcbace1 (Table3 word reads, Wide `Table3::EMPTY`, `isw_int` / `iswctype_view` int tail calls),
25dd7198 (MSRV 1.85), 6d363572 / 51eeed97 (run 1 handoff, D-5.7 assigned), 9633a833 (run 2: `_l` handle test), then
this handoff and report.

## Decisions in force
D-5.7 (s05, last row: the change), D-7.8, D-7.13. Profile of the base path: run 1 (below, unchanged).

## Run 2
* Step 0 on the inherited head: clippy rc 0, static rc 0 (shiki).
* Why tcwl-plants2 went red: plant iswalpha-l-handle (`__iswalpha_l` reading the thread's locale) SURVIVED. Two
  gaps: no test called the `__*_l` aliases (separate entries from `iswalpha_l`), and locale_ctype_l's `_l`
  comparisons always ran with the thread in "C", so a "C" handle was indistinguishable from the thread's locale.
  Fix (9633a833): `agree` calls every `_l` form AND its `__*_l` alias (classes, towupper/towlower,
  wctype/iswctype) while the thread runs in a locale whose wide classes differ from the handle's (C.UTF-8 for
  the C object; C for the C.UTF-8 and en_US.UTF-8 objects): both directions.
* Plants (handoff/TIMING-COV-WCTYPE-L-plants.txt, 6 plants, the two handle plants now on locale_ctype_l):
  tcwl2-plants 6 CAUGHT, 0 SURVIVED, no WARN.
* Gate tcwl2-gate (shiki, head 9633a833): direct tier 7157 passed / 0 failed (8 suites, 1 skipped: liella-abi, no
  test matched), conformance 2/0, peak 14G, rc 0; then the cbench_locale stage and counts.
* Counts (shiki, Haswell-EP, instructions per call, base e0c6b146 = tcwl2-base vs head = tcwl2-gate, glibc):
  iswctype_en_us 76 -> 73 (43); iswalpha_l_en_us 70 -> 66 (47); iswalpha_en_us 61 -> 57 (49); iswalpha_utf8
  66 -> 64 (49); towupper_l 39 = 39; nl_langinfo_l 35 = 35 (28); fnmatch_casefold_en_32 7236 -> 6101;
  newlocale_free, fnmatch_en_us, iconv rows equal or fewer. Target (glibc's count) NOT reached: about 1.4-1.7x left.
* strcasecmp_{c,utf8,en}_256: 250.1 -> 285.1 instructions. NOT the change: strcasecmp's code is identical on base and
  head except one field offset (0x688 -> 0x728, the view grew 160 bytes: `[Table3; 16]` for
  `[Option<Table3>; 16]`), and the compare is the unchanged asm kernel `liella_machine_casecmp_ord_*`. The bench's
  static `case_a`/`case_b[257]` moved with Liella's data size: base case_a at page offset 0xf60, head case_b at 0xf00,
  so each side has a different operand crossing a 4 KiB page and the kernel takes its page-cross path for it.
  RULING NEEDED (bench fix, not in this story): give the bench strings an alignment (`_Alignas(512)`) so the
  rows stop measuring the bench's data layout.
* Timing (cbench_locale, staged on shiki: base e0c6b146 and head 9633a833, run.sh = 3 interleaved rounds of
  `bench --compare --no-build --runs 3`, medians by ~/work/TIMING-COV-WCTYPE-L-2/tsum.py; ns per call, head/base):
  Ivy Bridge (kotori:tcwl2-ivb): iswctype_en_us 9.72 -> 9.20 (0.947; glibc 4.65), iswalpha_l_en_us 9.47 -> 9.15
  (0.966; glibc 7.12), iswalpha_en_us 8.19 -> 8.30 (1.013), fnmatch_casefold_en_32 0.797, fnmatch_en_us 0.938,
  every other row 0.985-1.001 except strcasecmp_{c,en,utf8}_256 1.130 (the layout artifact above).
  N100 (nozomi:tcwl2-n100): iswctype_en_us 7.69 -> 7.61 (0.990; glibc 4.18), iswalpha_l_en_us 7.27 -> 7.35 (1.011;
  glibc 5.02), iswalpha_en_us 1.003, fnmatch_casefold 0.794, fnmatch_en_us 0.928, strcasecmp_*_256 1.110 (artifact),
  the rest 1.000-1.005. Bookings 113 and 114 cancelled.
* Verdict: small gains on the target rows (IVB -5% / -3%, N100 about +-1%), target (glibc) not reached; the only
  slower rows are the strcasecmp layout artifact. This family has had several tweak stories (PERF-WCTYPE-L-2/-3,
  this one) with small gains: by the RUT RULE the next iswctype / `_l` story should be a RETHINK, not a tweak.

## Not done / next step
1. N100 timing (nozomi:tcwl2-n100, queued behind booking #111) if still pending: read it with
   `python3 -I ~/work/TIMING-COV-WCTYPE-L-2/tsum.py ~/logs/tcwl2-n100.log` on nozomi; cancel booking 114 after.
2. The remaining gap to glibc (about 23-30 instructions on iswctype / iswalpha_l): `view_of` (about 9 instructions,
   null/alignment), iswctype's desc decode (u16 test plus a power-of-two test), the iswalpha_utf8 arm (`wclasses`
   computes all classes), nl_langinfo_l (35 vs 28). Profile first (run 1's profile below is of the base).
3. The bench alignment ruling above.

## Run 1 profile (base, `__iswalpha_l` hit path, CJK U+4Exx in en_US)
About 58 instructions: about 9 resolve the `locale_t`, 2 push/pop for the cold calls, about 37 the three-level walk
(bound test plus a checked byte-range read per level, two absent-block tests, an `Option` test). `__iswctype`:
about 15 more (`push rbx`, `desc <= 0xffff`, a 5-instruction power-of-two test, a stride-40 index).

Plan rows: D-5.7 (plan/07-decisions/s05), D-7.13, handoff/PERF-WCTYPE-ENUS.md.
