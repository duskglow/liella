# Phase 14 saturation review packet (PRE14-SATURATION-PACKET)

Compiled 2026-11-05 from `docs/saturation/` (the intro 00-intro.md and sections 1 to 19) and the V-4 appendix in `docs/benchmarks.md` on this
branch (next `a6fc34c2`, 2026-11-05). No code changed, nothing decided, no recommendations (the coordinator adds them).

## How to read this

* **i / t** = Liella / glibc, V-4 page: instructions per call (Ivy Bridge host A), and time on host A / host B.
  Below 1.00 is faster. `-` = the row has no value in V-4; "not in V-4" = no such row.
* **Important caveat on every ratio:** V-4 measured two Ivy Bridge hosts only (no AVX2). Every register claim that was
  measured on a Zen 1 / Skylake-SP / Haswell AVX2 path cannot be confirmed or refuted by V-4; the V-4 number is the
  SSE2/fallback path. Where the claim is about the AVX2 path, the grade is on the claim's own numbers and the V-4 column is
  context (marked "other path").
* **Grades:** PROVEN = first-principles bound with numbers the current ratio meets. MEASURED = attempts recorded, no
  bound. STALE = cited numbers no longer match V-4, or the code changed since (the register itself flags many rows
  "recheck"). OPEN = still above 1.05 by instructions with no bound. The grade is mine from reading the entries; where a
  register file's own text was only skimmed (13 to 16: see "Not covered") that is said.
* Date = the register entry's date (the rule: the register's intro, docs/saturation/00-intro.md, gives 2026-10-23 for sections 01 to 12; later
  sections carry their own).

## String and memory kernels

| Row(s) | V-4 i / t (A / B) | Claim and how derived | Tried (negative) | Date | Grade |
|---|---|---|---|---|---|
| `strstr_hit_64` | 2.17 / - | "short gap stays": fixed cost of the AVX2 pair kernel; cycles 50.6 vs 22.2 (Zen 1), i 174 vs 87 | base 220 to 174 gained; no cheaper setup recorded | 10-23 | OPEN (V-4 other path; 2.3x) |
| `strstr_miss_64` | 1.24 / - | same entry | same | 10-23 | OPEN |
| `strstr_hit_4k` | 1.06 / - | AVX2 pair filter 28 instr per 128 B (0.22/B); parity not reached; one-stream filter estimated 18/128 B, not built | STRSTR-HIT-2 worse, removed (D-4.49); 64B loop to 128B loop 1299 to 1144 vs glibc 830 | 10-23 / PERF-STRSTR-HIT-4K | MEASURED (counts are Zen 1 AVX2; the 18/128 estimate is unbuilt) |
| `strstr_miss_4k` | 0.36 / - | "already below glibc" | - | 10-23 | PROVEN as parity-or-better (V-4 0.36) |
| `strncmp_diff8` | 1.44 / 1.75 | gate only, no claim; Zen 1 41.0 vs 34.0; SSE2 shape 1.41 still | STRCMP-HEAD (32B block +5 cycles; VEX.128 +9 instr; split +3 cycles), STRCMP-FIXED net loss | 10-23, 11-0x (reg. 18) | OPEN |
| `strncmp_eq16`, `strncmp_names64` | 1.01 / 1.27; 1.07 / 1.00 | gate only | as above | 10-23 | MEASURED |
| `strcmp_late_256` | 1.46 / 1.02 | register 18: gap REAL at SSE2 (198 to 183 vs 120 glibc, 1.50 after PERF-STRCMP-NONAVX2); default level 111 = 111 | PERF-STRCMP-LOOP 181.1 to 175.1 | 10-23 | OPEN (SSE2); AVX2 path at parity per entry |
| `strcmp_equal_16`, `strcmp_names` | 1.06 / 1.00; 0.94 / 0.76 | 10-07 claims (t 2.10, 1.49) are superseded; register 10 shows them moved | STRCMP-HEAD/FIXED | 10-07 | STALE (old numbers); current rows at/below 1.06 |
| `memchr_16` | 1.58 / - ; `_16_mix` 1.33 / 1.24 | "below or at glibc" (Zen 1: 25.4 vs 28.4) | none recorded | 10-20 | STALE for SSE2 (V-4 1.58) |
| `memchr_64`, `strlen_*`, `strnlen_*`, `rawmemchr_*` | 0.74 to 0.92 (strlen 16/64 0.65) | below glibc | SSE2-GAP net loss (strlen_64 Zen 1 38.4 vs 33.4 flagged ABOVE; V-4 SSE2 0.65) | 10-20/23 | PROVEN at/below parity on V-4; Zen 1 "above" rows are other path |
| `strchr_*`, `strchrnul_*` 16/64 | 1.04 to 1.07 hit, 0.74 to 1.04 miss / t 1.16 to 1.56 | within 0 to 2 instr of glibc (Zen 1); register 13 row: reproduced 41.0/34.0 etc. on Zen 1 | 3-instr scalar head +2 on head differences, rejected | 10-20/23 | MEASURED; t above 1.3 on V-4 |
| `memrchr_64_hit` / `memrchr_4k` | 1.11 / 1.28; 0.86 / 0.89 | PERF-MEMRCHR-NONAVX2 closed 4k below glibc | - | 11-0x | MEASURED (hit row still 1.11) |
| `memcmp_16` / `_64` / `_128` | 1.08 / 0.90; 1.02 / 1.00; 0.94 / 1.22 | "at parity, instruction floor of load/compare/branch ladder", Zen 1 39.4 vs 39.4 | MEM-ENTRY-2 fused flag test +1 uop; MEMCMP-EQ rejected (D-4.47); 3-instr head rejected | 10-20 | MEASURED; register 13 itself withdraws it as a time claim (t 1.27, 1.61 SSE2 then; V-4 `memcmp_128` t 1.22) |
| `memcmp_144`, `memcmp_256` | 1.01 / 1.32; 1.10 / 1.40 | 129-256 B "CLOSED to 1.02" on Zen 1 (70.5 to 58.4 vs 57.4) | - | 11-04 | MEASURED (other path); V-4 SSE2 1.10 |
| `memcmp` 33-64 AVX2 pre-check | not a V-4 row | keep 8-byte scalar pre-check: cheaper whenever > 4/13 (31%) of calls differ in head; sort-C 99.9996% differ in bytes 0-7; +10.91% instr:u when dropped (160.2M = 17.8M x 9) | bd02ee3a reverted | 10-2x | PROVEN (static path counts + measured rise agree) |
| `__memcmpeq_head_128/_1k`, `bcmp_head_128/_1k` | 1.85 / 1.84; 1.56 / 1.92 | register 15: "tried, no code change, row OPEN" (28.4 vs 20.4 Zen 1) | head rows 39.4 to 28.4 on one attempt, equal 128B worse | 10-24 | OPEN |
| `__memcmpeq_tail_64/_128/_32`, `bcmp_eq_64/_1k` | 1.38, 1.11, 1.10; 1.34, 1.21 | 65-2048 classes spend ~10 of the 10-11 gap on NULL tests + class dispatch; folding is a ruling (MEMCMP-MID-4 torn read) | 32 B on AVX2 block: +5 cycles (MEMCMP-MID-4) | 11-0x | OPEN |
| `memcpy_4k`, `memcpy_64k`, `memset_4k`, `memset_64k` | 26.33, 1.85, 2.46, 1.89 / t 0.86, 0.96, 1.03, 1.00 | `rep movsb`/`rep stosb` retire as few instr; 64 KiB `rep movsb` +60% cost reverted; MEMCPY-POLICY "do not redo" | MEM-IFUNC, SMALLCOPY, MEMCPY-NONAVX2-THRESHOLD | 10-2x | MEASURED (instr ratio meaningless for ERMS; time is parity); V-4 page says cause not investigated |
| `memcpy_*`/`memset_*`/`memmove_*` 16, 64, 128, 256 | 0.63 to 0.96 / t 0.60 to 1.17 | Zen 1 20.4 vs 19.4 etc. "no saturation claim" | entry-ladder, fused flag | 10-2x | STALE (Zen 1 gaps of 1 to 3 instr; V-4 at/below 1.00) |
| `strdup` | not in V-4 | floor is malloc + free; saves 12-16 on short, costs 3-12 on 40+ B | no code shipped | ledger | MEASURED |
| `strcasecmp_c_256`, `_en_256`, `_utf8_256`; `_c_16` | 1.16 / 0.96; 0.98 / 1.02 | register 07: AVX2 kernel built; SSE2 gap 31 instr (1.10x) at 256 B, "BOUND" is unverified guess at fewer instr per block | CASECMP, STRCASECMP-SHORT | 10-23/24 | MEASURED (bound not first-principles); 1.16 > 1.05 |
| `strcoll_utf8_16`, `strcoll_en_us_utf8_16` | 0.58 / 0.73; 0.88 / 0.78 | PERF-STRCOLL-ELEMENTS-2 "parity reached" (0.88); PERF-STRCOLL-UTF8 NEGATIVE earlier (1.29) superseded | the 1.29 negative | 10-23 | PROVEN (reached, V-4 0.58/0.88) |
| `getline_80`, `getline_long` | 0.99 / -; 1.20 / - | remainder is `rep movsb` vs vector copy: memcpy policy; gl_80 +2.9 instr accepted | 128 B step -43% front path | 10-2x | OPEN (long 1.20); STALE vs MEMCPY-POLICY |

## stdio

| Row(s) | V-4 i / t | Claim and derivation | Tried (negative) | Date | Grade |
|---|---|---|---|---|---|
| `fwrite_4k` | 2.05 / 0.99 | register 02/13: stdio-layer cost; nothing keeps D-12.19 and reaches parity: seal + both bounds ~15 instr; each trim saves 1 to 3 instr and needs a ruling or new `unsafe` | trims (a) to (c) listed, (c) negative | 10-23 | MEASURED; OPEN by instr, parity in time |
| `getc_unlocked`, `putc_unlocked`, `getc`, `putc`, `fwrite_unlocked_*` | 1.00, 0.90, 0.82, 0.75/0.70, 0.73 / t 0.71 to 1.00 | register 13 row 8 had i 1.47 to 2.89 / t 1.35 to 1.82; V-4 now at/below | - | 10-23 | STALE (earlier 2.89 gone) |
| `printf %02x` on a stream; `snprintf_d`, `_x08`, `_ld_large`, `_e` | not in V-4 as named; `snprintf_*` 0.84 / 0.91 / 0.84 / 0.96 | ahead of glibc | PRINTF-PARSE, PRINTF-FLOAT `79fb043d` reverted | 10-20 | PROVEN as parity-or-better |
| `snprintf_e` 3.28 old | 0.96 / 0.91 | "not at a limit" | - | 10-07 | STALE (moved) |

## malloc

| Row(s) | V-4 i / t | Claim and derivation | Tried (negative) | Date | Grade |
|---|---|---|---|---|---|
| `malloc_free_16..1024` | 0.83 / 1.11 to 1.18 | ahead by instr (111 vs 135, Zen 1). The t 1.16 gap: on Skylake-SP cycles 45.1 vs 46.3 (parity, 0 store-forward, 0 branch miss), "specific to Ivy Bridge or cbench_malloc" (unconfirmed) | HEAP-META (4-5 instr) rejected; HEAP-HINT; frameless realloc entry; `K_HINT` conditional store +3 instr no gain | 10-23 | MEASURED; t 1.11-1.18 on V-4 hosts is the only time gap, cause unconfirmed, HEAP-META-DENSE changed the allocator after the claim |
| `realloc_grow_x2` | 1.11 / 1.06 | no register row | - | - | OPEN |
| `realloc_within_usable` | 0.98 / 1.19 | register 15 N3-4 (Zen 1: i 1.11, cyc 1.22/1.31); V-4 instr 0.98 | GUESS: general resize path | 11-0x | STALE (instr moved), time 1.19 open |
| `realloc_null_24` / `_moving` / `_grow_16step` | 0.85 / 0.60 / 0.54 | ahead | REALLOC-CHAIN/GROW | 10-2x | PROVEN as ahead |
| debug check folded out of fast path | not a row | 4 instr per pair; closed deliberately (concurrency risk) | none | 10-2x | MEASURED (a decision, not a bound) |

## fs walk

| Row(s) | V-4 i / t | Claim and derivation | Tried (negative) | Date | Grade |
|---|---|---|---|---|---|
| `readdir_entry`, `opendir_readdir_closedir_1000`, `readdir_entry_flat` | 1.00 / 0.94 to 0.95 | parity, kernel `getdents64` dominated; 75.9 vs 64.8 (Zen 1, user instr) | DIR-SYSCALLS: zero-fill-free buffer needs `unsafe` (RULING NEEDED, never ruled) | 10-23 | PROVEN at parity (instr 1.00); the old 1.17 gap is gone |
| `realpath_5` | 1.04 / - | deep path: 1.0019 per component over 64 (46 instr), kernel `readlinkat` per component; residual not ablated | - | 10-2x | PROVEN (slope over 64 components) |
| `scandir_alphasort_1000` | 0.97 / - | parity 1.003, kernel `openat` walk identical | 1.29 before comparator change | 10-2x | PROVEN |
| `fts_physical_1000`, `nftw_phys_1000` | 1.09, 1.10 / - | register 02: "one copy is the floor", note predates FTS-SHARED-PATH; flat ~1.2x, not at a limit | `ddbcff99` +9 per entry no gain, reverted | 10-2x | STALE (floor claim); OPEN above 1.05 |
| `fts_entry_flat` / `fts_entry_deep2k` | 0.87, 0.31 | register 19: 0.84 and 0.27 | - | 11-0x | PROVEN as ahead |
| `posix_spawn*`, `execvp_missing` | 1.00 to 1.03; 1.01 / - | kernel-dominated; execvp 1.013 (3 dirs), 1.010 to 1.012 (200 dirs) | - | 10-23 | PROVEN (within 1.05) |
| `getcwd` | 1.02 | - | - | - | n/a |

## locale and wctype

| Row(s) | V-4 i / t | Claim and derivation | Tried (negative) | Date | Grade |
|---|---|---|---|---|---|
| `iswalpha_l_en_us`, `iswctype_en_us`, `towupper_l_en_us`, `iswalpha_en_us` | 2.32 / 1.91; 2.42 (page); 1.74 / 1.57; 2.04 / 1.43 | register 15: "first-use fill inlined" GUESS; PERF-WCTYPE-L-2 NEGATIVE, NOT KEPT; next steps (a) lazy BMP bit cache, (b) flat delta table NOT tried | PERF-WCTYPE-L-2; LOCALE-CTYPE-L +6% | 11-04 | OPEN (NEGATIVE for one technique); up to 35% worse than 10-23 per V-4 page |
| `iswalpha_utf8`, `towupper_utf8`, `wcwidth_utf8` | 1.39 / 0.95; 1.32 / 0.80; 1.16 / 0.79 | "faster in time, more instructions: IPC advantage" | WCTYPE-LOCALE no gain | 10-07 | STALE (10-07 t 0.89 vs V-4 0.95 etc.); OPEN by instr |
| `isalpha_c`, `toupper_c` | 1.44 / 1.00; 1.05 / 1.00 | parity in time | LOCALE-CTYPE-L reverted | 10-07 | MEASURED (time parity; instr 1.44) |
| `mbrtowc_*`, `strftime_*`, `snprintf_f*` | 0.20 to 0.89; 0.44 to 1.01 | old "not at a limit" t 1.655/1.78-2.26 | LV-4, STRFTIME-FAST reverted | 10-07 | STALE (moved far below) |
| `gettext_found`, `gettext_language_steady`, `_alternating` | 1.34 / 1.01; 2.17 / 1.62; 1.29 / 1.16 | memo hit +0.4-0.6% accepted; "not removed" list text may be rewritten in place, removing it saves ~40 of 110 instr (steady 1.58 to ~1.50) but needs `liella-boundary` | GETTEXT-LANGUAGE-ALIGNED-COMPARE-2 NEGATIVE (fused compare slower) | 11-04 | OPEN (steady 2.17); the 1.58 figure is STALE vs V-4 |
| `setlocale` alias, locale data page faults | not rows | alias cache gains nothing; pre-read raised faults | ALIAS-FAST, RC2-FAULT | 10-2x | MEASURED |
| cold `first_*`/`second_*` rows | not in V-4 (cold) | first setlocale en_US ~58k vs 22k; repeated bindtextdomain+gettext 14,896 vs 2,405 | - | 10-23 | OPEN |
| `uselocale(0)`, `localtime_r_new_york` | not in V-4 / 0.07 | ahead | - | 10-20 | PROVEN as ahead |

## regex, glob, fnmatch, wordexp

| Row(s) | V-4 i / t | Claim and derivation | Tried (negative) | Date | Grade |
|---|---|---|---|---|---|
| `regexec_backref` | 1.06 / 1.17 | X-3a: a DFA cannot decide back-references; span filter removes at most ~10% (estimate, unmeasured); capture rows already 0.07 to 0.27 | PERF-REGEXEC-BACKREF: class-nomatch CLOSED, backref OPEN; REGEX-SED +4.4% "do not retry"; RX-T4C; REGEX-WL | 11-04 | OPEN (backref 1.06; the ~10% is an estimate) |
| `re_mt4_shared`, `re_mt4_distinct` | 1.56, 1.55 / 0.02, 0.23 | instr rise across four threads; time far below glibc | - | - | OPEN by instr, time faster |
| `regexec_class_nomatch_1k` | 0.95 / 1.14 | "CLOSED" | - | 10-24 | PROVEN (instr); t 1.14 |
| capture/sed/grep rows | 0.07 to 0.56 | old t 3.1-5.8 | - | 10-07 | STALE (moved) |
| sed-utf8 one-byte prefilter | workload, not a row | +0.40% instr accepted (ruling #2333); probe shows a start never repeats | - | 11-04 | MEASURED (accepted cost, not a bound) |
| `fnmatch_*` | 0.28 to 0.84; `fnmatch_pathname_nomatch` 1.00 | PERF-FNMATCH-CLASS: Zen 1 i 1.34 before (the entry's after figure was not read) | - | 10-23 | STALE (V-4 `fnmatch_names_class` 0.51) |
| `wordexp_quoted`, `_tilde_var`, `_param_forms`, `_arith`, `_split_var` | 1.47, 1.33, 1.18, 1.17, 1.07 / 1.73, 1.52, 1.35, 1.46, 1.11 | OPT-STAGE1 "no gain, reverted"; "no design for a larger change"; X-5 profile: larger change not justified | OPT-STAGE1 | 10-2x | OPEN (all above 1.05, no bound) |
| `glob` rows | not in V-4 as named | - | - | - | n/a |

## libm

| Row(s) | V-4 i | Claim and derivation | Tried | Date | Grade |
|---|---|---|---|---|---|
| 169 rows (88 double, 41 float, 40 long double) | median 9.7 double, 18.1 float, 504 long double; 165 slower by instr; only `log10f` 0.84 | Not a saturation claim. Register 16 (f64 exp/log/pow breakdown) lists steps by estimated instr (FMA ~335, reduction ~220 and others); #6 and #7 DONE in MATH-M6-F64-GAP-ASSEMBLY-2. Float and long double gaps "not measured to a cause" (V-4 page) | Math stories held or landing | 11-0x | OPEN |

## other

| Row(s) | V-4 i / t | Claim and derivation | Tried | Date | Grade |
|---|---|---|---|---|---|
| `qsort_c_cmp_1024` (also `_e4`, `_e8`, `_e64`) | 1.51 / 1.07 to 1.09; 1.51; 1.47; 1.29 | register 19: 47.9 vs 31.7 instr per comparison (cycles 24.2 vs 21.8, 1.11); the gap is 8 check instr + slice building per comparison, safe-Rust sort; comparing by offset needs a core design change, "ask the owner first" | design change not built | 10-23 | OPEN (instr), ruling pending |
| `qsort_*_e16`, `_e24` | 0.87, 0.86 | ahead | - | 10-23 | PROVEN as ahead |
| tar -c page layout (`du`, `ls -lR` +1 page) | not a row | no buffer policy removes a layout shift | TAR-HEAP | ledger | MEASURED |
| `fread`/`fwrite`/`fclose` discard, `ualarm`, `mallinfo`, `ecvt` | n/a | spec requires; deliberate differences | - | - | not a perf claim |
| `fork_exit_wait` 1.06, `waitpid_echild` 1.09, `sys_close_ebadf`, `sprintf_chk_ls_nlink` (+8% since 10-23) | per V-4 page | no register row / "kernel-shared or small" | - | - | OPEN |

## Weakest claims (STALE, then MEASURED with the biggest ratios)

1. **`memcpy_4k` 26.33 / `memset_4k` 2.46 / `memcpy_64k` 1.85 / `memset_64k` 1.89** by instructions: the register
   says `rep movsb` retires few instructions; no per-phase split of the 26x exists on V-4 (MEASURED, time at parity).
2. **wctype `_l_en_us` / `iswctype_en_us` / `iswalpha_en_us` (1.74 to 2.42)**: the "first-use fill" cause is a GUESS in
   the register; one technique NEGATIVE; two next steps untried (OPEN).
3. **`gettext_language_steady` 2.17, `gettext_found` 1.34**: STALE (register cites 1.58); fix proposal needs unsafe in
   boundary.
4. **`strstr_hit_64` 2.17**: register's "gap stays" is a Zen 1 AVX2 cycle claim (50.6 vs 22.2); V-4 gives the SSE2
   value; same story, different path.
5. **`qsort_c_cmp_1024*` 1.29 to 1.51**: bound is a design (compare by offset), not a measured limit.
6. **`__memcmpeq_head_*`/`bcmp_head_*` 1.56 to 1.85**: attempted, "row OPEN".
7. **`strncmp_diff8` 1.44, `strcmp_late_256` 1.46, `memchr_16` 1.58**: register 03 gives "within 0-2 instr" for these
   on Zen 1; STALE for the SSE2 path.
8. **`malloc_free_*` t 1.11 to 1.18** with i 0.83: the only cause analysis is a Skylake-SP result that does not
   reproduce the gap (unconfirmed).
9. **`wordexp_*`** 1.07 to 1.47: "no design for a larger change".
10. **`fts_physical_1000`/`nftw_phys_1000` 1.09/1.10**: "floor" claim predates FTS-SHARED-PATH.
11. **`isalpha_c` instr 1.44** (time parity claim holds); **`towupper_utf8` etc.** time fell from 10-07 values.

## Not covered / limits of this packet

* Registers 13, 14, 15 and 16 (about 110 KiB) were searched by keyword for bounds and negative results, not read in
  full; stories not matched by that search (for example `getline`, `strlen` mid-range, `strnlen` 100-300 B at 1.15-1.39
  SSE2, PERF-STRNLEN-MID-3, putc `D-12.19` trim list details) are cited only at the level shown.
* V-4 has no AVX2 host: every AVX2-path claim is graded on its own numbers, not on V-4.
* I did not re-verify any ratio by running a bench (planning story, no jobs).
