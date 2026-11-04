## 13. Next: perf stories from the 10-23 numbers (PERF-NEXT-BATCH, 2026-10-23)

PLANNING ONLY: nothing was measured or changed for this section. Gaps are the section 10 and 12 numbers (`bench/parity-measure.tsv` rows of 10-23, and for the `memref`/`malloc` rows `handoff/bench-refresh-1023-logs/<host>-compare.log`). Owner bar: at least glibc's speed; parity is not a ceiling.

**How hot.** No per-function workload call counts are stored in the tree (`wl_count` gives totals per workload, not per function). Hotness is the share of samples in the Liella profiles `validation/results/profiles/*.top` (milestone 2, 2026-10-04) and `validation/results/m6/profiles/*.top` (2026-10-05). Those builds predate the optimization thread, so a share says *the workload calls this a lot*, not how big the gap is today. Rank = gap x that share; where no profile names the function the hotness is "none seen" and the row ranks low whatever its gap.

**Host caveat that applies to every row.** Ivy Bridge host A and Ivy Bridge host B are Ivy Bridge without AVX2. The section 3 instruction limits were set on Zen 1 (AVX2 kernels). An Ivy Bridge gap may be the non-AVX2 kernel or dispatch only; every proof below therefore measures on Ivy Bridge host A (or Ivy Bridge host B) AND on an AVX2 timing host (AVX2 timing host, booked), and a change must not slow either.

**Milestone 6** (track M, `plan/06-phases/04-track-m-parallel-from-p2.md`) covers libm speed (the fast path before the double-double one). No libm row is in the 10-23 tables, so nothing below overlaps it.

### 13.1 "At the limit" claims the 10-23 numbers no longer support

| Register row (section) | Claim | 10-23 number (host) | What to do |
|---|---|---|---|
| `memcmp_16/64/128` (3) | "at parity", hardware instruction floor | t 0.90, 1.27, 1.61; i 1.08, 1.22, 1.11; `memcmp_64_mix` t 1.23 (Ivy Bridge host A) | Claim holds for Zen 1 instruction counts only; withdraw it as a time claim. Story NB-1. |
| `strchr`/`strchrnul`/`memrchr` 16 and 64 (3) | "within 0 to 2 instr of glibc", hardware | strchr 1.35 to 1.83, strchrnul 1.19 to 1.74, memrchr_64_hit 1.28 (Ivy Bridge host A); i up to 1.38 | Not within 2 instructions on this host (dI 8 to 21). Stories NB-3, NB-4. |
| `malloc` + `free` pair (4) | "ahead of glibc" | t 1.16, i 0.83 (Ivy Bridge host A) | Fewer instructions, more time: ahead by count only. Story NB-6. |
| `iswalpha`, `towupper` UTF-8 (5) | "faster in time, more instructions: IPC advantage" | iswalpha 1.26, towupper 1.03; `iswalpha_l_en_us` 1.54, `towupper_l_en_us` 1.19 (Ivy Bridge host B/Ivy Bridge host A) | The IPC advantage is gone. Story NB-7. |

Still supported by the 10-23 numbers: `readdir` (t 0.95), `isalpha`/`toupper` C (t 1.00), `snprintf`, `strftime`, `localtime_r`, `mbrtowc` 1 and 2 byte (all below 1.00). Not re-checked here: `memcpy_128`/`memmove_128` parity, `strlen_16`/`memchr_*` "below or at glibc" (no 10-23 row was found in the lines read).

### 13.2 Stories, ranked

Every story: **proof** = paired in-process counters before and after, `cargo xtask bench --compare --counters --runs 5` on a booked Ivy Bridge host A/Ivy Bridge host B AND a booked AVX2 timing host (AVX2), spread under 10% or the row is rerun; the direct-tier gate (`cargo xtask affected --run`, label `<story>-gate`); plants on the changed code with `msg:` fields (`.S` kernels: anchored to the function label, tests named explicitly: `-p liella-boundary --test machine_scanners` / `small_copy`); `wl_count --base` for the workloads named in the row (no workload slower: workload gate). **Verified job** that every story starts with: `cargo xtask static` plus `cargo xtask clippy` on its role, as a `jobrun` job labelled `<story>-static`. Causes are from the notes cited; **GUESS** marks a cause no note measures.

| # | Id | Rows (host) | Gap | Hot in | Likely cause | Size | Depends on |
|---|---|---|---|---|---|---|---|
| 1 | PERF-MEMCMP-NONAVX2 | `memcmp_64`, `_128`, `_64_mix` (Ivy Bridge host A) | t 1.27, 1.61, 1.23; dI 9, 7, 9 | sort-C (`liella_machine_memcmp` 57%), sort-n | GUESS: the non-AVX2 class ladder; Zen 1 counts (section 3) are the AVX2 path. MEMCMP-MID-4's torn-read bound (section 2) stays. | M | none |
| 2 | PERF-STRLEN-NONAVX2 | `strlen_64`, `strnlen_64`, `rawmemchr_64` (Ivy Bridge host A) | t 1.20, 1.41, 1.28; i 0.91, 1.39, 1.35 | make-n (`strlen` 10%), bash-loop 9%, sort-utf8 12%, ls-lR | strlen: fewer instructions but slower (i 0.91): GUESS a dependency chain or unaligned head; strnlen/rawmemchr: extra instructions (SSE2-GAP was a net loss, section 3: do not redo that head). | M | shares the scanner code with #3; do #2 first |
| 3 | PERF-STRCHR-NONAVX2 | `strchr_16/64` hit and miss, `strchrnul_16/64` (Ivy Bridge host A) | t 1.19 to 1.83; dI -2 to 21 | make-n (`strchr` 1.5%); bash, sed GUESS (no profile line) | GUESS: as #2; the 16-byte rows with dI 0 to 10 at 1.35 to 1.83 point at latency, not count. STR-CHR-CMP-SHORT's 3-instr scalar head was rejected (do not redo). | M | #2 |
| 4 | PERF-MEMRCHR-NONAVX2 | `memrchr_64_hit`, `memrchr_4k` (Ivy Bridge host A) | t 1.28, 1.13 | grep-E (`memrchr` 4.7%), grep-F 3.1% | GUESS: the 4k row (1.13, i 1.05) says the main loop, not only the head. | S | #2 (same kernel file) |
| 5 | PERF-STRCMP-NONAVX2 | `strcmp_late_256`, `strcmp_equal_16`, `strncmp_diff8`, `strncmp_eq16` (Ivy Bridge host A) | t 1.24, 1.10, 1.75, 1.21; i 1.62, 1.03, 1.37, 0.99 | make-n (`strcmp` 3.5%), bash-loop 2.9% | late-256: loop instruction count (i 1.62); diff8: settle path. STRCMP-HEAD and STRCMP-FIXED closed the head ideas (section 3: do not redo). | M | none |
| 6 | PERF-HEAP-PAIR-TIME | `malloc`+`free` pair (Ivy Bridge host A) | t 1.16 with i 0.83 | bash-loop (heap 20%), gawk-regex (free 12%, malloc 8%), make-n | GUESS: a store-forwarding or branch cost (fewer instructions, more cycles); HEAP-META and the frameless entry were rejected (section 4: do not redo). First step: cycles and branch misses per pair. RUN 2 (Skylake-SP host, section 4): no gap on Skylake-SP; conditional hint stores measured negative. Open: the Ivy Bridge cycles on Ivy Bridge host A. | M | none |
| 7 | PERF-WCTYPE-L | `iswctype_en_us`, `iswalpha_l_en_us`, `iswalpha_utf8`, `towupper_l_en_us`, `nl_langinfo_l_en_us` (Ivy Bridge host B) | t 2.80, 1.54, 1.26, 1.19, 1.56; dI 56, 35, 31, 27, 16 | grep-i-utf8, sed-utf8 (`[[:alpha:]]`), wc -m, gawk-utf8 GUESS (no profile line names them) | instruction count (i 1.5 to 2.3): the `_l` and `iswctype` entries take a longer path than `iswalpha`; WCTYPE-LOCALE's outlined walk gave no gain (section 5: do not redo). **MEASURED 2026-10-23 (PERF-WCTYPE-L, Skylake-SP host Skylake-SP AVX2, `bench --compare --counters --runs 3 cbench_locale`, instr/op Liella vs glibc dyn):** iswctype_en_us 99.1/43.1, iswalpha_l_en_us 83.1/47.1, iswalpha_en_us 74.1/49.1, iswalpha_utf8 81.1/49.1, towupper_l_en_us 67.1/39.1, nl_langinfo_l_en_us 44.1/28.0: the same dI as Ivy Bridge host B, so the gap is host- and dispatch-independent (pure Rust, no AVX2 kernel; forcing non-AVX2 is moot). Disassembly of our `__iswctype` (libc.so.6): on the CJK path (wc > 0xff, the bench's) the entry saves 7 registers (7 push + 7 pop = 14 instr) that only the inlined first-use fill of the Latin-1 class cache (`Latin::class` -> `w_classes_file`, a 12-mask loop, `crates/core/src/locale/view.rs`) needs; the rest is the bounds-checked Table3 walk (`table3.rs` `block`/`bit`). NEXT (not measured): make the `Latin::class`/`Latin::map` first-use fill `#[cold] #[inline(never)]` and recount; this outlines the cold fill, not the walk WCTYPE-LOCALE outlined. | M | none |
| 8 | PERF-GETC-PUTC | `getc_unlocked`, `getc`, `putc_unlocked`, `putc`; `fwrite_4k` (Ivy Bridge host A, Ivy Bridge host B) | t 1.82, 1.35, 1.56, 1.39, 1.05; i 2.89, 1.88, 2.10, 1.47, 2.21 | make-n (`getc_raw` 1.0%), cut-tr-uniq (`fwrite` 5.2% + 2.6% + 2.5%) | GUESS: the call goes through the Rust stdio layer where glibc tests the buffer pointer in a few instructions (i up to 2.9x). **Measured (PERF-GETC-PUTC, 13.6): guess refuted; the gap is D-12.19's validation (seal, bounds, orientation), a ruled floor.** | M | none |
| 9 | PERF-STRCOLL-UTF8 | `strcoll_en_us_utf8_16` (Ivy Bridge host A) | t 1.11, i 1.30 | sort-utf8 (`strcoll` 3.8% + loop 2.0%), ls-lR (`xstrcoll_name` 1.6%) | instruction count; milestone 2 profile: `strlen` + `compare_bytes` (validation/profiles.md item 6). | M | none |
| 10 | PERF-GETLINE-SHORT | `getline_80`, `getline_long` (Ivy Bridge host A) | t 1.30, 1.10; i 1.29, 1.25 | sed-utf8 (`getdelim` 0.4%); sort, cut, wc read by line GUESS | long: the `rep movsb` copy (STRSTR-GETLINE, section 3); 80: per-call fixed cost GUESS. **Measured (PERF-GETLINE-SHORT/-2, instructions:u per call, AVX2 forced off for both libraries):** the 80 guess is CONFIRMED: the gap was getdelim's two-pass find+copy without AVX2. An SSE2 fused scan+copy (D-12.19) cuts gl_80 from 275.4 to 204.8 (glibc 195.0; Skylake-SP host before/after, Haswell-EP host after identical); gl_long 5641.8 -> 5693.3 (+0.9%) and gl_long_win 2713.9 -> 2777.7 (+2.3%); default dispatch unchanged (184.6 / 3575). The long guess (`rep movsb`) is REFUTED: the gap is the refill path (open). **Workload counts (PERF-GETLINE-SHORT-3, Zen 1 host, wl_count base 8afbbd5e):** none worse, outputs SAME; AVX2 off sed-utf8 0.9850, sed-s 0.9961 new/base; sort-C, sort-utf8, wc, cut-tr-uniq flat in both modes (their getline GUESS refuted); default mode sed 0.9995 / 0.9999. | S | MEMCPY-POLICY (done) |
| 11 | PERF-STRCASECMP-SSE2 | `strcasecmp_c_16`, `strncasecmp_c_64` (Ivy Bridge host B; the 64 row NOISY 13%) | t 1.60, 1.18 | none seen | the AVX2 `casecmp_ascii` kernel was never built (section 7); this host needs an SSE2 one. | M | #3 (same scanner style) |
| 12 | PERF-WORDEXP | six `wordexp_*` rows (Ivy Bridge host A) | t 1.14 to 1.77 (`plain_8` 0.72) | none seen (no release workload calls wordexp) | instruction count (i 1.14 to 1.52); "no design for a larger change" (section 6). | L | none; rank low until a workload calls it |

**Row 9 result (PERF-STRCOLL-UTF8, 2026-10-23): NEGATIVE, dropped.** Gap reproduced on Haswell-EP host (E5-2697 v3) with `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2` (both builds), `cargo xtask bench --compare --counters --runs 3 cbench_opt`: `strcoll_en_us_utf8_16` instr/op Liella 45621.6, glibc 35251.4 (i 1.29), t 1.08. The row's cause is wrong for this bench: the milestone 2 `strlen` + `compare_bytes` profile is sort-utf8 in C.UTF-8, which now takes the code-point path (`order_direct`). The en_US pair differs at `!` vs `.` after a non-ASCII prefix (`é`, `€`), and a call costs ~45k instructions in both libraries, so the time is in the general path (`collate::compare`), not the entry. Tried: let `decide_first_flag` decide after a non-ASCII shared prefix when no catalog element continues with an ASCII byte (a per-slot flag computed in `fill` from the `tablemb` lists: literal bytes and range bounds all >= 0x80, ranges parting only at their last byte; GBK keeps the guard); correct by the locale_data oracle (8 locales, partial characters and contraction starts before the difference; en_US decided > 12800 pairs after a non-ASCII prefix, zh_CN.GBK none). Result: row unchanged (45628.5 instr/op), because `!` and `.` have no rank (no plain first unit; GUESS: IGNORE at the first level), so the pair still ties at the first level. Kept on the branch history as commit 87d70f44 (reverted in the next commit) for a story that has a workload with accented names differing at a letter. Measured profile of the row (Haswell-EP host, `perf record -e instructions:u`, CBENCH_ONLY=strcoll_en_us_utf8_16, SSE2 forced): `collate::Key<&[u8], Memo>::raw_next` 43.2%, `<&[u8] as Text>::element` 34.5% (the per-lead-byte `extramb` list walk), `Memo<Source<&[u8]>>::at` 13.5%, `Locale::compare_bytes` 5.6%, `strlen` 0.12%, `strdiff` 0.16%: the `strlen` hypothesis is refuted; the cost is the key pass over the levels after a first-level tie and the element lookups. Next step: cut element lookups (index the long lead-byte lists, or find each element once for all levels) and the per-unit cost of `raw_next`; measure element calls per strcoll first.

Below the cut (ranked low by hotness or noise, for the record): `regexec_backref` 1.29, `regexec_class_nomatch_1k` 1.12, `fnmatch_names_class` 1.26 (no current profile names them; the sed-s regex share was measured before milestone 3's regex work); `gettext_language_steady` t 1.26 / cycles 1.44 (NOISY 11%: re-measure first); `iconv_utf8_ascii_translit_4k` 1.32 (i 0.87: time, not count); `fts_physical_1000` 1.08 and `nftw_phys_1000` 1.08 (du 82% kernel); `strstr` short (NOISY 46%, section 3 claim "the short gap stays" is not decided by a noisy row: re-measure).

### 13.3 PERF-STRLEN-NONAVX2 (2026-10-23, Ivy Bridge host A, measured)

Paired `xtask bench --compare --no-build --counters --runs 5 cbench_memref` on Ivy Bridge host A (Ivy Bridge, booked host idle), A = branch base, B = 93846aaf, job `perf-strlen-nonavx2` (its A/B interleave step failed: libc.a was not shipped; both compare runs completed, logs Ivy Bridge host A:~/work/perf-strlen-nonavx2/compare{A,B}.log).

* A reproduced the row: strlen_64 t 1.20 / i 0.91, strnlen_64 1.41 / 1.39, rawmemchr_64 1.28 / 1.34.
* Cause found (strlen): not a dependency chain or the unaligned head. On the SSE2 level a 64-79 B string missed the [s+32, s+64) fold and went through the aligned 64-byte group fold, then the locate step (62.4 instr/op incl. the bench loop). Branch-miss and store-forwarding counters were NOT read (instructions and cycles only).
* Change: after the fold miss, one aligned 16-byte block b = (s + 64) & -16 (holds s + 64); a miss enters the body as before. B: strlen_64 11.35 -> 7.42 ns, t 1.20 -> **0.79**, i 62.4 -> 44.3 (0.65). Cost: +6 instr per call for strings of 80 B or more (strlen_4k 580 -> 586 instr, time 88.03 -> 88.04 ns, unchanged). strlen_16 and every other row unchanged. AVX2 level: the code is unreachable there (the AVX2 head or the 4032 test takes every AVX2 call), so no AVX2 timing host run was made.
* Still open: strnlen_64 (i 1.39) and rawmemchr_64 (i 1.34) are instruction-count gaps, untouched; workload counts (wl_count) for make-n, bash-loop, sort-utf8 and ls-lR not run.

### 13.4 PERF-STRCHR-NONAVX2 (2026-10-23, Zen 1 host, AVX2 forced off, instructions:u in-process)

* METHOD FINDING: `cargo xtask bench` clears the environment (BENCH-FIXED-ENV, xtask/src/bench.rs), so `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2` given to `xtask bench` does NOT reach the benchmark: such a run measures the default (AVX2) dispatch. Force it by running the built binaries with `env -i PATH=... HOME=/home/cbench CBENCH_COUNTERS=1 GLIBC_TUNABLES=...` (Zen 1 host:~/work/perf-strchr-nonavx2/m.sh). Any earlier "SSE2 forced" number taken through `xtask bench` should be rechecked.
* Gap reproduced (base, mean of 3, Liella vs glibc static, instr/op): strchr_16_hit 41.0/34.0, strchr_64_hit 75.1/58.0, strchr_64_miss 78.1/58.0, strchrnul_16_hit 39.0/29.0, strchrnul_64 73.1/53.0; the 16_miss rows already beat glibc (56/58, 51/53). It matches Ivy Bridge host A (41.1, 75.1, 73.1), so it is the instruction count of the SSE2 path (flag, second page test, aligned blocks up to a 64-byte boundary, strchr's re-read), not a latency the host adds. Branch-miss and store-forwarding counters were not read.
* Change (c67f8888): on the SSE2 level with [s, s + 64) in s's page, four unaligned 16-byte blocks with one exit branch each. The entry's (s - 1) & 4095 compare decides "SSE2 level and in-page"; strchr decides c or NUL from the two masks of the loaded register. After-numbers: job Zen 1 host:perf-strchr-nonavx2-pc-new2 (~/work/perf-strchr-nonavx2/new.txt), NOT READ in this run.
* Re-measured (PERF-STRCHR-NONAVX2-2, Zen 1 host, `cargo xtask bench --compare --counters --runs 3 --tunables glibc.cpu.hwcaps=-AVX2 cbench_hot`, BENCH-TUNABLES; base = ca804fb8 without c67f8888, jobs Zen 1 host:perf-strchr-nonavx2-2-pc-base / -pc-new). Liella/glibc instr/op, before -> after: strchr_16_hit 41.0/32.0 -> 32.0/32.0, strchr_16_miss 56.0/56.0 -> 40.0/56.0, strchr_64_hit 75.1/56.0 -> 56.1/56.0, strchr_64_miss 78.1/56.0 -> 56.1/56.0, strchrnul_16_hit 39.0/29.0 -> 29.0/29.0, strchrnul_16_miss 51.0/53.0 -> 37.0/53.0, strchrnul_64_hit and _miss 73.1/53.0 -> 53.1/53.0. The forced numbers match run 1's env -i script, so run 1's base was a real SSE2 measurement (glibc's side reads 2 fewer through `--tunables` than through env -i). Default (AVX2) rows unchanged: strchr 28.0/29.0 (16), 35.1/38.0 (64); strchrnul 25.0/27.0, 32.1/36.0.
* Verdict: KEPT. The SSE2 gap was instruction count only: after the change the hit rows equal glibc's count and the miss rows are 16 below it. Branch-miss and store-forwarding counters were not read; with equal counts they were not needed to explain the row (a remaining time gap on Ivy Bridge host A, if any, is for the release A/B run).
* make-n (validation/wl_count.sh, Zen 1 host, base ca804fb8, job perf-strchr-nonavx2-2-wlc): AVX2 forced off (WLC_ENV) instructions:u 793.0M -> 780.6M (new/base 0.9843; glibc 936.6M, env check "changed (ok)"); default 759.76M -> 760.09M (1.0004), output SAME. The default +0.04% is NOT variance: a COUNT_FRESH=1 re-count (job perf-strchr-nonavx2-2-wlc-fresh) gives the same 760.09M vs 759.76M (+329k instr:u). Not located (budget). Hypotheses, unmeasured: the cmp/jae the AVX2 level now executes on its page-end entry (.Lschrnul_pre/.Lschr_pre, reached when (s - 1) & 4095 >= 4032), or a layout shift of the code after the inserted heads. Next step: wl_percall on strchr/strchrnul for make-n by default; if it is the page-end compare, route the AVX2 level past it (e.g. a separate pre label for the AVX2 entry). OPEN for a ruling: SSE2 -12.4M vs default +0.33M on make-n.

### 13.5 PERF-STRNLEN-RAWMEMCHR (2026-10-23, Zen 1 host; counted on Skylake-SP host by PERF-STRNLEN-RAWMEMCHR-2, kept)

* Change (commit 23eceba6): strlen's 13.3 fix applied to SSE2 strnlen and rawmemchr. After the [s + 32, s + 64) fold misses, one aligned 16-byte block b = (s + 64) & -16 (strnlen: loaded only when b - s < max, else max). On a miss the code enters the aligned body as before. strnlen also gets strlen's .Lstrnlen_third: one compare covers the level and the page test, a two-load fold replaces the three-load [s + 16, s + 64) fold, the page offset is reused, and r11 is set up only on the body path. The old non-masked SSE2 head could no longer be reached and was removed. The AVX2 level is unchanged (ecx >= 4033 there, so .Lstrnlen_third goes to .Lstrnlen_main).
* Tests and plants: both plants the brief names are covered by tests that already exist in machine_scanners: strnlen with max = len + 1 at page offset 4032 (b = the next page), and rawmemchr with the match on the page's last byte at offset 4031. Plants job Zen 1 host:perf-strnlen-rawmemchr-plants (p1 bound off by one, p2 unaligned rawmemchr block, p3/p4 hit index).
* Run 1's percall and wl_count jobs never ran (exit 2: base not primed on Zen 1 host). Measured by PERF-STRNLEN-RAWMEMCHR-2 on Skylake-SP host, head 777069d2 vs base 2d02a9f8 (NEXT-ARCH-AUDIT-2 tip, without this change).
* Per-call instructions:u, SSE2 forced (GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 for both libcs), wl_percall job Skylake-SP host:perf-strnlen-rawmemchr-2-percall2, sd 0 everywhere. Columns are glibc / base / new (new/glibc):
  strnlen_64 77 / 89 / **66 (0.86)**; strnlen_64_o5 and strnlen_70_o5 77 / 89 / 66 (0.86); strnlen_64_max64 62 / 71 / 59 (0.95); strnlen_40 55 / 83 / 59 (1.07); strnlen_100 77 / 101 / 104 (1.35); strnlen_300 113 / 150 / 152 (1.35); strnlen_4k 809 / 672 / 674 (0.83).
  rawmemchr_64 53 / 63 / **49 (0.92)**; rawmemchr_64_o5 53 / 63 / 49 (0.92); rawmemchr_40 42 / 45 / 45 (1.07); rawmemchr_100 65 / 69 / 76 (1.17); rawmemchr_300 136 / 129 / 136 (1.00); rawmemchr_4k 1006 / 912 / 919 (0.91).
  The 64-byte targets are now below glibc. As 13.3 predicted for strlen, strings past s + 80 pay for the extra block: strnlen +2..3, rawmemchr +7 instructions per call. Open gap, unchanged by this story: strnlen at 100-300 B (1.35, was 1.31-1.33) and rawmemchr_100 (1.17). That is the aligned body's cost per 64 B, not the head.
* Workloads (wl_count job Skylake-SP host:perf-strnlen-rawmemchr-2-wlc2; instructions:u new / base, new / glibc; output SAME as base and glibc in all four): make-n 1.0000 / 0.858; bash-loop 1.0000 / 0.954; sort-utf8 1.0000 / 0.933; ls-lR 1.0001 / 0.934 (+60k of 848M instructions, 0.007%). No workload changes measurably: the release mix seldom calls strnlen or rawmemchr on SSE2 sizes near 64 B.
* Plants (Skylake-SP host:perf-strnlen-rawmemchr-2-plants, fresh RAM copy, baseline PASS): p1 to p4 all CAUGHT with their expected message.

### 13.6 PERF-GETC-PUTC (row 8; numbered 12.5 on next before the PERF-STRNLEN-BODY entries merged): measured, no code change (2026-10-23)

**Reproduced.** Instructions per call were measured on Broadwell-EP host with `cargo xtask bench --compare --counters --runs 3 cbench_hot cbench_stdio` (job `perf-getc-putc-base-percall2`, Broadwell-EP host = Broadwell E5-2699 v4, AVX2, NOT booked: instruction counts only; the time ratios below are indicative). Liella/glibc: `getc_unlocked` 44.3/15.3 (i 2.89), `putc_unlocked` 40.2/19.1 (2.10), `getc` 51.3/27.3 (1.88), `putc` 47.2/32.1 (1.47). These are the 10-23 Ivy Bridge host A/Ivy Bridge host B counts to the decimal. Indicative time on Broadwell-EP host: 1.42, 1.65, 1.22, 1.36. `fwrite_4k` is i 1.14 on Broadwell-EP host, against 2.21 on Ivy Bridge host B: the AVX2 copy kernel accounts for that difference. That points at the non-AVX2 copy, not at stdio (see rows 1 and 2).

**Where the instructions go** (disassembly of `target/liella/libc.so.6` on the role tree, release build). `getc_unlocked`/`fgetc_unlocked` has no call, no stack frame and no spill. The hot path is 34 straight-line instructions ending in `ret`, and every slow case is one `jmp` to `getc_slow`. They break down as:
- fp null and 8-alignment: 5
- read window `ptr < end`: 3
- `_codecvt` record above 8 and aligned: 5
- seal = fp xor the `%fs` pointer guard: 4
- `_mode < 0`: 3
- record buffer non-null: 3
- offset below the engine's `rend`: 4
- offset inside the buffer slice: 2
- the byte, `ptr + 1` stored, `ret`: 4
- one hoisted slow-path argument (`mov $2,%al`): 1

`putc_unlocked` is 28: the same checks without `_mode` (the write window is closed on wide streams) and without `rend`. By measurement, glibc's call costs about 5 instructions: its 15.3 per loop iteration, minus the roughly 10 of the bench loop and indirect call (Liella's 44.3 minus its 34). **So about 27 of the 34 instructions are the validation that D-12.19 requires**: the owner ruling of 2026-10-07 (sealed record), the slice bound and the orientation test of STDIO-2. The "Rust stdio layer" is fully inlined.

**Time.** In the 10-23 table (Ivy Bridge host B), `getc_unlocked` runs at about 3.3 instructions per cycle in Liella's build (44.4 instr, 13.6 cycles) against about 2.1 in glibc's (15.3 instr, 7.4 cycles). Both versions carry the same loop dependency, `_IO_read_ptr` load, +1, store, so the extra time is the issue throughput of the check instructions. It is not a stall: branch misses and store forwarding were not measured separately, but no instruction outside that chain depends on the stored pointer. Each check uop removed is worth about 0.25 to 0.3 cycles.

**What would close it, and why it was not done.** Nothing that keeps D-12.19 can reach parity: the seal and both bounds alone are about 15 instructions. Each trim below needs a ruling or an engine invariant, and saves 1 to 3 instructions (about 3 to 9% of the call):
- (a) Drop the `_mode` test (3) by keeping the published read window closed on wide streams, as the write window already is. This is an engine-wide invariant change (every `store` transition).
- (b) Drop either the `rend` or the slice bound (2) by relying on the engine invariant `rend <= buf_len`. This needs `get_unchecked`, i.e. new `unsafe`.
- (c) Drop the buffer-null test (3) by relying on `buf == 0 => rend == 0`, ordered before the slice is built.
- (d) Move the hoisted `mov $2,%al` (1) off the hot path, which is only a code-layout change.

A booked Ivy Bridge host A/AVX2 timing host proof was not run, because no change was made. Do not redo: the "Rust layer" hypothesis, and looking for calls or frames on these paths (there are none). Ranking: `make-n` (`getc_raw` 1.0%) and `cut-tr-uniq` (`fwrite`) are the only workloads named. At about 0.3 cycles per removed uop, (a) to (d) together are worth at most about 2.5 cycles of about 13.6 per `getc`.

### 13.7 PERF-STRNLEN-BODY run 1 (2026-10-23, Broadwell-EP host, measured; #1478; its scan.S change was taken over by 13.4, on top of 13.5's block)

Method: `cargo xtask bench --tunables` (BENCH-TUNABLES) is not on next yet, so instructions:u per call were read in-process (perf_event_open around 20000 calls minus 10000, mean of alignments 0..63 step 9) by a probe linked static against the role's `libc.a` and dynamic against glibc 2.39, SSE2 level forced in both by `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2` (the counts change, so the level was really forced). Broadwell-EP host = Xeon E5-2699 v4 (AVX2). Jobs Broadwell-EP host:perf-strnlen-body-base / -after / -after2; probe and script Broadwell-EP host:~/work/perf-strnlen-body/.

* Cause (measured): at 1000 and 4000 B Liella was already at or below glibc (strnlen 0.98 / 0.82, rawmemchr 0.92 / 0.91): the 128-byte loop is not the gap. The gap was the step after the 64-byte head: the first aligned group re-checked the s & 63 bytes the head had done, sending 100-200 B strings to the loop and its two-step exit; and the body's step to the 128-aligned loop went back 64 bytes (a group checked twice) when the next group was already 128-aligned.
* Change: with [s, s + 128) in s's page (page offset <= 3968, which also means the SSE2 level), check [s + 64, s + 128) as one unaligned group located from the prefix-fold registers, then go on at P = (s + 128) & -64 (the loop directly when P is 128-aligned).
* SSE2 instr/call, base -> after (glibc): strnlen 64 97.8 -> 74.0 (77.6), 100 114.8 -> 84.0 (83.6), 150 128.4 -> 121.4 (93.5), 200 131.9 -> 124.9 (102.5), 300 148.9 -> 141.9 (120.5), 1000 248.4 -> 241.4 (252.5), 4000 669.3 -> 662.3 (815.0), 16 unchanged 45.0 (55.0). rawmemchr 64 68.0 -> 63.0 (54.8), 100 85.3 -> 69.0 (71.1), 150 100.3 -> 101.3 (103.4), 200 103.5 -> 104.5 (115.4), 300 128.1 -> 129.1 (139.0), 1000 279.5 -> 280.5 (302.9), 4000 910.5 -> 911.5 (1006.0): +1 per call from 150 B (the page test), still below glibc. AVX2 level: every row identical before and after (the code is unreachable there).
* Still open (owner bar not met): strnlen 150-300 B is 1.30 / 1.22 / 1.18 x glibc, rawmemchr_64 1.15 x. GUESS (not profiled): strnlen's loop exit re-loads the 64-byte half with anyz64 and then locates with reloads (find64, up to 4 x 6 instructions); a prefix-fold exit as rawmemchr's SSE2-MID loop has would save the re-load. rawmemchr_64: the 64 B string's NUL lies at s + 64, found by the unaligned group, so its cost is the head (4 broadcast + 3 tests). Cycles were NOT measured (counts only; no timing host booked). wl_count (make-n, bash-loop, sort-utf8, ls-lR) NOT run: the default level never reaches the changed code and its counts are identical in the probe.

### 13.8 PERF-STRNLEN-BODY-3/-4 (2026-10-23, Skylake-SP host, counted; #1478, #1544): owner bar NOT met for SSE2 strnlen 100-300 B

Change (on next's 64-79 B block): after a miss in b, check [q, q + 64) with q = b + 16 when the page offset is <= 3952,
locate from the fold registers, then go on at (q + 64) & -64; the SSE2 strnlen loop exit locates from registers.
instructions:u per call, wl_percall (spec strnlen_body.c: offsets 5/37, strnlen max = len + 64), base f34ea29f1 (next),
SSE2 forced with GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 (both sides). glibc / base / new:
strnlen 64: 80/66/66; 100: 80/104/97; 150 o5: 92/128/126; 150 o37: 92/139/128; 200 o5: 104/123/127 (WORSE +4);
200 o37: 104/135/129; 300 o5: 116/157/146; 300 o37: 128/147/147; 1000: 248/243/237.
rawmemchr 64: 51/49/49; 100: 63/76/72; 150 o5: 98/106/107; 150 o37: 105/110/111; 300: 135/137/138 (+1 each).
AVX2 (default): base = new on all 14 rows (code unreachable there).
Negative: strnlen 100-300 B stays 1.15-1.39x glibc at SSE2; the remaining gap is the head + 64-79 B block + q group
(three separately located steps before the loop), not the loop body (1000 B is below glibc). Closing it needs one
fewer located step before the 128-byte loop (merge the q group into the b block for page offsets <= 3952), a GUESS
not tried here (budget). strnlen 200 o5 +4 and rawmemchr 150/300 +1 are the cost of the q-group page test.

### 13.9 PERF-STRNLEN-MID-2 (2026-10-23, Haswell-EP host, counted; #1622, #1624): AVX2 strnlen/rawmemchr 100-300 B

Change (AVX2 level only): strnlen tests max <= 64 once at entry (that path keeps the bounded head), so max > 64 with
s's page offset <= 3871 checks six aligned 32-byte blocks after the 64-byte head with no bound tests (all in s's page),
each located directly, then the loop at (a + 192) & -128. rawmemchr peels the first loop group as two 64-byte halves
tested with vptest. instructions:u per call, wl_percall (spec strnlen_body.c, COUNT_FRESH=1, one level at a time),
glibc / before / after. AVX2: strnlen 64 46/45/44, 100 47/56/47, 150 51/84/51 (o5 and o37), 200 65/85/59,
300 o5 83/96/95, 300 o37 95/97/96, 1000 155/157/156; rawmemchr 64 35/35/35, 100 39/39/39, 150 43/60/52,
300 74/71/70. No row slower. SSE2 rows identical before and after (job Haswell-EP host:perf-strnlen-mid-2-pc4).
Still open: AVX2 strnlen 300 o5 1.14x (the loop's first group re-checks up to 64 bytes the chain did; at 300 B the
NUL lies in the loop's second group); rawmemchr 150 1.21x (glibc reaches 150 B in one located step past its head;
here head + two blocks + one half, about 9 instructions more). A one-block chain for rawmemchr reaches 150 B parity but
costs +6 at 300 B (#1624), not taken. SSE2 b/q check removal (#1622) NOT built in this run (budget).

### 13.10 PERF-STRNLEN-MID-3 (2026-10-23, Zen 1 host, counted; #1622, #1640, #1642): SSE2 strnlen b/q bound tests removed

Change (SSE2 level only): the b step tests max against 64 (no b - s in r8), the q group [q, q + 64) loads with no r11
test (it lies in s's page; a found index is clamped, a clean group leaves the loop's first bound test to return max),
r11 = s + max is set only on the way to the loop or the old body, and the q-hit clamp is inlined. instructions:u per
call, wl_percall (spec strnlen_body.c, COUNT_FRESH=1, one level at a time), glibc / before (run 2, Haswell-EP host pc4) / after
(Zen 1 host perf-strnlen-mid-3-pc-sse2): strnlen 64 80/66/65, 100 80/97/88, 150 o5 92/126/122, 150 o37 92/128/124,
200 o5 104/127/123, 200 o37 104/129/125, 300 o5 116/146/142, 300 o37 128/147/143, 1000 248/237/233. rawmemchr rows
unchanged (49/72/107/111/138). AVX2 rows identical to 13.9 (job perf-strnlen-mid-3-pc-avx2). No row slower.
Remaining gaps, not chased (#1642), with cause:
- SSE2 strnlen 100-300 B still 1.10-1.35x glibc. 16-byte vectors need four loads per 64 bytes, and every stage that
  is bounded (head, b, q, body/loop entry) pays a bound test, a located exit and an alignment step; the rejected
  q-into-b merge (#1622) added 12 on the 64-79 B rows. Closing the rest needs a different head (fewer, wider stages
  with one bound test), not another removed instruction.
- AVX2 strnlen 300 o5 1.14x: the loop's first group re-checks up to 64 bytes the six-block chain did, and the NUL lies
  in the loop's second group.
- AVX2 rawmemchr 150 B 1.21x: head + two blocks + one peeled half, about 9 instructions more than one located step; the
  one-block chain reaching parity costs +6 at 300 B (#1624), not taken.
Test: machine_scanners now runs offsets 0..127 from a 128-aligned base (was 0..63 of an unaligned Vec), so a loop group
that starts 128-aligned past the head is exercised; plant p8 (rawmemchr peeled half-1 check), which survived in 12.8,
is now caught. That test binary runs in about 9 s on Zen 1 host.

### 13.11 PERF-GETC-PUTC-2: trims (c) and (d) (ruling #1239, 2026-10-23)

- **(d) done** (commit `b68f962d`). `getc_c` now tail-calls `getc_slow_general` (no `ReadFront` argument) for the general case. Before the change, the Zen 1 host role build's `fgetc_unlocked` began with `mov $0x2,%al` (the hoisted `ReadFront::General`), then 33 more instructions to `ret`. The expected saving is 1 instruction per call. The after-disassembly, the per-call count and the make-n / cut-tr-uniq counts are in jobs `perf-getc-putc-2-chain2` (Zen 1 host). They were NOT READ when this was written.
- **(c) not done: negative, no gain without new `unsafe`.** In the disassembly, the buffer-null test (`mov 0x100(%rdx),%rsi; test; je`) is the explicit `ext.buf == 0` test merged with the `addr == 0` guard of `conv::bytes_at_mut`, which `Env::bytes` → `tb::stream_bytes` reaches. The load is needed anyway. Removing the explicit test leaves the guard in the slice constructor, so the same `test`/`je` stays (inferred from the codegen; not built separately). Saving these 2 instructions needs a slice constructor that trusts `buf != 0` whenever `buf_len != 0`. That is the invariant `ensure_buffer`/`setvbuf` keep, but relying on it is new unchecked `unsafe`, the HELD category (b). Leave it to the owner's review of D-12.19 together with (a) and (b).

### 13.12 PERF-MEMRCHR-NONAVX2 (2026-10-23, Zen 1 host, measured on instructions:u)

Forced SSE2 by running the built `cbench_hot` binaries directly with `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2` (both libraries). `xtask bench` clears the environment (BENCH-FIXED-ENV), so the tunable never reaches it: run through xtask, the rows were the AVX2 numbers. In-process instructions:u per call, base (origin/next) -> 88a1ff34, glibc dynamic in brackets: memrchr_64_hit 36.0 -> **31.0** [28.0]; memrchr_64 53.0 -> 48.0 [53.0]; memrchr_4k 1200.7 -> **983.7** [1144.7]. AVX2 (default) unchanged: 27.0, 33.0, 459.5 -> 458.4. Base reproduces Ivy Bridge host A's 36/28 exactly.
* Causes found (from the code path, counted): the head recomputed s + n - 1 and ran a 4-instruction page test before its first 16-byte look (now `testl $0xfc0` on the entry's r8, 5 instructions fewer); the 4k row was the 64-byte SSE2 loop (18 instr / 64 B), now 128-byte blocks (29 instr / 128 B, the AVX2 block shape). Branch-miss and store-forwarding counters NOT read; no timing run (release A/B covers it).
* Open: memrchr_64_hit is still +3 over glibc (entry tests 4 + AVX2 mask test 3 + 4-instruction byte broadcast). No workload counts read yet (jobs Zen 1 host:perf-memrchr-nonavx2-wlc / -wlc-sse2, grep-E grep-F).

### PERF-FWRITE-4K (2026-11-04, Broadwell-EP host, instructions:u per call, `validation/percall/fwrite4k.c`, dynamic glibc, no code change)

`fwrite_4k` is the 4096-byte block written through an 8192-byte full buffer to `/dev/null`: Liella 798.7, glibc 645.0 (1.24 per call; the cbench ratio is 1.14). Per-call counts of the pieces (Liella / glibc):

| case | Liella | glibc |
|---|---|---|
| `fwrite_4k` (8 KiB buffer, every second call straddles the buffer end) | 798.7 | 645.0 |
| `fwrite_4k_win` (64 KiB buffer: window copy, one flush in fifteen) | 562.0 | 559.3 |
| `fwrite_4k_thru` (4 KiB buffer: written through, no copy) | 507.0 | 455.5 |
| `memcpy` 4088 B, destination 8 bytes off alignment | 434.0 | 422.0 |
| `memcpy` 4096 B, aligned | 83.0 | 426.0 |
| `write(fd, p, 0)` wrapper | 53.0 | 21.0 |

Findings. (1) The window path is at parity (1.00): the copy is not the gap. (2) The 8-byte stream reserve puts every copy 8 bytes off alignment, so the copy is the AVX2 loop (434, glibc 422), not the 83-instruction `rep movsb` an aligned copy gets; moving that threshold is the memcpy policy (MEMCPY-POLICY-2, D-4.14: `rep movsb` measured slower in cycles at 2 to 4 KiB off the 64-byte placement), not a stdio change, and was not touched. (3) The excess is in the straddling call (about 470 instructions over a window call, glibc about 160 over): the engine's set-up and write-back around `State::write`, the write wrapper (+32 per syscall) and the split copy. These are 25 to 30 instructions per piece, spread over several functions; no single site accounts for more than the 16 per call of the write wrapper. Not changed: a change would be speculative from counts alone (sampling on the VM loses most samples) and the row cannot reach parity without the copy threshold. Next candidate if wanted: a straddle fast path in `write_raw` (copy the head, drain, copy the tail without the engine's `run` set-up), and the `write` wrapper's 53 instructions.
