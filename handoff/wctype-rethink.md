# WCTYPE-RETHINK: wide-character classification, rethought (planning; RUT RULE trigger 1)

**State:** branch `task/wctype-rethink` from origin/next `acc8cfcd5`; this commit adds this file and `handoff/wctype-rethink/`
(probe `wcprobe.c`, job scripts `run.sh`/`trun.sh`, analysis `an.py`/`tan.py`, result summaries). No product code changed.
**Recommendation: adopt design A** (one two-stage class-word table per LC_CTYPE; every entry = table pointer + 2 loads + mask test).
Owner/maintainer decisions are in the hand-back as RULING NEEDED. Proposed decision rows: section 7 (ids to be assigned).

## 1. Inputs (read before any code)
* Rows (origin/next `handoff/timing-coverage-1107/table.md`, counters-mei.txt): `iswctype_en_us` i 1.77 / t 2.09 (Ivy Bridge),
  1.84 (N100); `iswalpha_l_en_us` i 1.49 / t 1.33, 1.45; `iswalpha_utf8` i 1.35 / t 0.96, 1.06. Plain `iswalpha` en_US 0.88 / 0.99.
* Tried before (handoffs PERF-WCTYPE-ENUS, -L-2, -L-3, -UTF8-REST-3; docs/saturation/15): inline hit paths and `#[inline(always)]`
  (kept, gained); Table3 accessor micro-changes (5 of 6 rows slower, reverted); prologue splits (no change). Untried: (a) per-view
  lazy BMP cache, (b) flat case-delta table, (c) Table3 offsets resolved once. TIMING-COV-WCTYPE-L-2 (ruling #3857): -5%/-3% IVB.
* Decision rows: D-5.1 to D-5.6 (s05), D-7.13 LV-4 (views). **D-5.7 does not exist on origin/next** (the brief names it).
* Spec: ISO C 7.30.2 / POSIX `iswctype` (a `wctype_t` is valid only for the LC_CTYPE it came from; invalidated by `setlocale`),
  so the descriptor encoding is Liella's choice. Data: Unicode 15.1 (D-5.4). Method source: The Unicode Standard 15.1, ch. 5.1
  "Data Structures for Character Conversion" (multistage tables). Hardware: Ivy Bridge (no AVX2/BMI2), N100, Skylake-SP.

## 2. Black-box measurement (Liella entries; glibc instructions/cycles/time only)
Probe `wcprobe.c`: each entry called through a function pointer over 256 inputs per range; per call = (loop with the entry - same
loop with an empty function of the same signature) / 4096, median of 5. Counters (instructions:u, cycles, taken branches, loads) on
the Skylake-SP build host (mei, loaded: cycles are indicative only), job `wctype-rethink-measure` (rc 0). Ranges: ascii 0x20-0x7E,
latin1 0xA0-0xFF, cjk U+4E00.., bmpmix (8 scripts, runs of 32), astral (emoji, Ext-B, math, Deseret), bmprnd (random BMP).
Full tables: `handoff/wctype-rethink/counts-skylake-sp.txt`. Medians over the 12 classes (class spread in brackets), net instr:

| locale | entry | ascii | latin1 | cjk | bmpmix | astral | Liella taken / loads |
|---|---|---|---|---|---|---|---|
| C | isw* | 51 vs 5 | 37 vs 15 | 37 vs 15 | 37 vs 15 | 37 vs 15 | 6 / 11 |
| C | iswctype | 57 vs 28 | 43 vs 8 | 43 vs 8 | 43 vs 8 | 43 vs 8 | 6 / 11 |
| C.UTF-8 | isw* | 55 vs 5 | 55 vs 36 | 55 vs 28 | 55 vs 31 | 55 vs 28 | 4 / 14 (every range) |
| C.UTF-8 | iswctype(_l) | 61 (67) vs 29 | 61 (67) vs 29 | 61 (67) vs 21 | 61 (67) vs 25 | 61 (67) vs 22 | 4 / 14 |
| en_US | isw* [18-48] | 20 vs 5 | 20 vs 36 | 38 vs 28 | 43 vs 31 | 38 vs 28 | 3-4 / 6-12 |
| en_US | isw*_l | 26 vs 4 | 26 vs 33 | 44 vs 25 | 49 vs 28 | 44 vs 25 | 3-4 / 6-12 |
| en_US | iswctype(_l) | 25 (31) vs 29 | 25 (31) vs 29 | 52 (58) vs 21 | 57 (63) vs 25 | 52 (58) vs 22 | 3-4 / 6-12 |
| en_US | towctrans | **319 vs 23** | **454 vs 23** | **445 vs 19** | **448 vs 20** | **443 vs 17** | 37-45 / 44-65 |

Cycles track instructions (e.g. en_US iswctype cjk 29 vs 11; towctrans 234 vs 12). Timing (section 4) is the verdict.

## 3. Designs (first principles; both prototyped)
Data facts (probe `stats`, Liella's own answers, all 1,114,112 code points x 12 classes): **13 distinct class words** in
C.UTF-8 and en_US.UTF-8; **en_US.UTF-8 = C.UTF-8 for every code point and class** (0 differences) on this reference system;
C differs (ASCII only). Building the words through the API costs 160-270 ms (so never eagerly per locale).

**Design A: one shared two-stage table of class words; `wctype_t` = the class bit (already Liella's encoding).**
`w = s2[(s1[c >> 8] << 8) | (c & 255)]; return w & mask` after `c < 0x110000`. The table pointer comes from one load (thread-block
word for the active LC_CTYPE, as D-5.5 does for the byte tables; a field of the locale object for `_l`). Per call, all ranges and
CPUs: ~13 (isw) / 15 (iswctype) instructions, 4 loads (pointer, length, stage 1, stage 2), 0 taken branches besides call/ret,
2 cache lines (a 64-B stage-1 line covers 8192 code points; a stage-2 line 32), constant shifts only (no BMI2 needed on Ivy
Bridge). Memory (shared rodata for built-ins, deduplicated leaves): leaf 256: 84.0 KB (147 leaves); leaf 128: 78.8 KB; 8-bit
palette leaves (13 words): 46.8 KB (+1 dependent L1 load); three-stage leaf 32: 40.2 KB (+1 load). C locale: 9.7 KB (2 leaves).
**Design B: Latin-1 class-word table + one bitmap trie per class (512-bit leaves = one cache line, shared by all classes).**
c < 256: one load. Else `leaf = s1[k][c >> 9]; bit = leaf_words[leaf*8 + (c>>6 & 7)] >> (c & 63) & 1` (k = class index; for
iswctype k = ctz(mask)). 5-9 instructions and 2 loads (1 taken branch) up to U+00FF; 18-22 instructions, 5 loads above; variable
shift (`bt` on Ivy Bridge). Memory 69.8 KB (266 shared leaves 17 KB + 52 KB per-class stage 1).
Considered and rejected without a prototype: sorted range lists with binary search (alpha has hundreds of ranges: ~10 dependent
compares and data-dependent branches per call); a per-thread last-block memo (adds a compare/branch and a store to save one load).

## 4. Prototype results (`wcprobe.c`, tables built from Liella's answers, verified: 0 mismatches in 13,369,368 checks per locale)
Counters (Skylake-SP, all 12 classes, every range, all three locales): A 13 / 15 instructions, 4 loads, 0 taken, 3.6-6.4 cycles;
B 5-9 instr (2.1-6.8 cycles) up to U+00FF, 18-22 instr and 10-15 cycles above.
**Timing, net ns per call, median of 15 interleaved rounds** (jobs nico:`wctype-rethink-ivb`, nozomi:`wctype-rethink-n100`, booked;
full table `handoff/wctype-rethink/timing-ivb-n100.txt`), cells Liella today / glibc / design A:

| CPU | locale | entry | ascii | latin1 | cjk | bmpmix | astral | bmprnd |
|---|---|---|---|---|---|---|---|---|
| Ivy Bridge | en_US | iswalpha | 4.82/0.89/0.89 | 4.81/7.11/0.87 | 5.86/7.09/0.89 | 5.72/7.10/0.88 | 5.62/6.51/0.87 | 7.50/7.01/0.89 |
| Ivy Bridge | en_US | iswalpha_l | 4.38/0.87/0.87 | 4.37/5.13/0.87 | 6.35/5.15/0.87 | 6.24/5.14/0.89 | 5.81/4.68/0.87 | 8.06/5.38/0.89 |
| Ivy Bridge | en_US | iswctype | 3.94/2.03/0.89 | 3.94/2.02/0.89 | 7.60/2.03/0.87 | 7.60/2.00/0.89 | 7.06/1.97/0.87 | 9.31/2.26/0.89 |
| Ivy Bridge | C.UTF-8 | iswalpha | 5.71/0.88/0.87 | 5.71/7.18/0.87 | 5.91/7.21/0.88 | 5.80/7.21/0.90 | 5.74/6.61/0.91 | 5.74/7.17/0.89 |
| N100 | en_US | iswctype | 2.10/2.51/0.83 | 2.10/2.51/0.83 | 6.21/2.52/0.83 | 6.23/2.51/0.85 | 5.86/2.64/0.84 | 8.57/2.82/0.84 |
| N100 | en_US | iswalpha_l | 2.51/2.09/1.04 | 2.51/3.37/1.04 | 5.05/3.37/1.05 | 5.05/3.36/1.04 | 4.91/3.53/1.04 | 6.94/3.67/1.04 |
| N100 | C.UTF-8 | iswalpha | 4.60/2.10/0.83 | 4.60/4.34/0.83 | 4.60/4.30/0.84 | 4.60/4.34/0.84 | 4.60/4.34/0.84 | 4.60/4.31/0.84 |

Design A runs 0.83-1.26 ns on every entry, range and locale on both CPUs: 0.12-0.50x glibc, except ascii isw*/isw*_l on Ivy
Bridge, which are at parity (0.96-1.00; glibc 0.87-0.93 ns). Liella today: 0.68-7.2x glibc (worst: ascii isw*/isw*_l 5.0-7.2x on
Ivy Bridge, iswctype cjk 3.7x).
B is 1.3-1.8 ns above U+00FF, so A wins. NOT VERIFIED: the Rust cost of resolving the table (view word, `_l` handle check), which
the prototype models as one TLS load / one field load; first-touch cost of lazily resolved blocks (D-NEW-2).

## 5. Current implementation (read only now) and why it costs what it does
* `LocaleView::isw` (crates/core/src/locale/view.rs:551): file-backed views test a Latin-1 cache (global `LATIN` slot, 2 checked
  loads), then for one class walk the locale file's per-class 3-level table (`in_class_bit`), else `isw_wide` out of line. Built-in
  views (C, C.UTF-8) call `ctype::wclasses` (crates/core/src/ctype.rs:114), which matches the locale kind and for C.UTF-8 derives
  the class word from Unicode properties on every call (`unicode::classes`): the flat 55 instructions / 14 loads, ASCII included.
* `iswctype` already takes the mask (`iswctype_view`, `Class(desc)`); locale-defined classes (`desc >= DEFINED`) go cold.
* `_l`: `view_of` checks the handle through `tb::locale_object` before the view (+6 instructions on every `_l` row).
* `towctrans` (crates/legacy/src/ctype_tables.rs:241 `trans_of`) recomputes every candidate map's descriptor to decode `desc`, then
  takes the non-view `ctype::wtrans`: 319-454 instructions in en_US (~20x glibc; MAGNITUDE trigger). Separate fix, brief 8.3.
Design A replaces the three paths (cache + per-class file walk, Unicode-property derivation, kind dispatch) with one lookup.

## 6. Recommendation
Adopt A: generated two-stage class-word tables (leaf 256) for C and C.UTF-8 in rodata; file-backed LC_CTYPE gets a lazily
resolved stage 1 whose blocks point at the built-in leaves when equal (en_US.UTF-8 here: all) or at per-locale leaves; the
thread block holds the active table pointer, the locale object its own. Expected (prototype + ~3-6 instructions of Rust
resolution): every isw*/iswctype row 15-22 instructions, about 1-1.5 ns on both timing CPUs (glibc 0.9-7.2 ns). Fix towctrans first.

## 7. Proposed decision rows (D-NEW; s05 ids reserved by the coordinator)
* **D-NEW-1 (adopt A):** wide classes come from one two-stage table of class words per LC_CTYPE (bit = `Class` bit; stage 1 by
  `c >> 8`, deduplicated 256-entry u16 leaves); `wctype_t` stays the class mask; every `isw*`, `isw*_l`, `iswctype(_l)` = table
  pointer (1 load) + 2 loads + mask test. Method: Unicode Standard 15.1 ch. 5.1 multistage tables. Evidence: this file, section 4.
* **D-NEW-2 (file locales):** per file-backed LC_CTYPE a stage 1 of `AtomicU16` (0 = unresolved; ~8.5 KB per locale); a miss
  computes the block's 256 words from the file's class tables and publishes (Release) an index into the built-in leaves when equal,
  else into a per-locale leaf pool; locale-defined classes keep the existing path. Eager build rejected (API build 160-270 ms).
* **D-NEW-3 (resolution):** the thread block holds the active LC_CTYPE's class-table pointer, kept current where D-5.5 keeps the
  byte-table words; the locale object holds its own for `_l` (extends D-5.5; maintainer ruling: it changes what `uselocale` stores).
* **D-NEW-4 (towctrans):** `wctrans_t` decode is O(1): the view stores its toupper/tolower descriptors once; `towctrans` compares
  against them and calls the view's `wtrans`; other maps keep the search.

## 8. Code briefs
1. **WCTYPE-TRIE-BUILTIN** (M): `cargo xtask unicode-gen` emits the C.UTF-8 and C class-word tables; built-in views read them.
   Test first: exhaustive equality, every code point x 12 classes, new table vs today's `wclasses` (C, C.UTF-8). Plants: stage-1
   shift, leaf offset, class bit. Counts `cbench_locale` + `wcprobe count`; timing on Ivy Bridge and N100 (book; `trun.sh`).
2. **WCTYPE-TRIE-FILE** (M/L, after 1): D-NEW-2 and D-NEW-3. Exhaustive equality vs the file walk (en_US.UTF-8 and the system
   C.utf8 file); a thread-race test on first touch; plants: unresolved marker, publish order. Measure first-touch cost per block.
3. **TOWCTRANS-DESC** (S, first: >10x): D-NEW-4; test `towctrans(c, wctrans(name))` == `towupper/towlower` for every code
   point in C, C.UTF-8, en_US.UTF-8. Expected 445 -> about 45-50 instructions (today's `towupper` en_US cjk: 45).
4. Later: the same two-stage shape for case deltas (`towupper` en_US cjk 1.24x Ivy Bridge, `towupper_l` 2.45x): PERF-WCTYPE-L-2 (b).

## 9. Not to redo / process
* Do not micro-tune Table3 walks or prologues (three stories, <5%). The prototype tables and verification are in `wcprobe.c`.
* Raw outputs stay on mei `~/work/WCTYPE-RETHINK/` (count-*.tsv) and the timing hosts (`t-*.tsv`). Bookings 115/116 cancelled.
* wcwidth and locale-defined classes (ja_JP-style) were not measured: only C, C.utf8 and en_US.utf8 are installed on the hosts.
