<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 7: Locale and text

* **Scope:**
  * §7.1–7.4 and §7.6, locale objects and information;
  * §5 full, Unicode classification and case mapping, and the `wctype`/`wctrans` names;
  * §6 full (37 symbols), except external iconv charsets beyond the built-in set;
  * the locale-sensitive parts of §4 (`strcoll`, `strxfrm`, `*casecmp`, `strverscmp`) and
    §8 (radix and grouping in `strtod` and friends).
* **Symbols:** about 190.
* **Clauses:** about 90.
* **Needs:** P6. Locale objects are shared across threads, and `uselocale` is per thread.
* **Builds:** IP-19 (Unicode tables generated from the UCD), IP-20 (locale model and
  loader; the locale-archive format is inspected under the approved format-inspection rule
  and recorded as spec Appendix D), the ctype ABI tables per locale (`__ctype_b_loc` and
  friends), and the
  collation engine (ISO 14651 multi-level).
* **Test surface:**
  * Class relations for all code points.
  * `strcoll` agreeing with `strcmp` over `strxfrm` keys (spec §4.3 guarantee).
  * `setlocale` composite names round-trip.
  * `newlocale` base consumption.
  * `mbrtowc` sentinel cases (-1, -2 and -3 for `mbrtoc16`).
  * iconv resumability across every split point, `//TRANSLIT`/`//IGNORE` reporting (D-6.4),
    and `E2BIG`/`EILSEQ`/`EINVAL` positions.
* **Exit:** §5, §6, §7.1–7.4, §7.6 and the locale parts of §4/§8 green. MB_CUR_MAX = 6 for
  UTF-8 locales (D-6.5).
* **Design first (owner rulings 2026-10-01):** named locales come from compiled data only
  (D-7.2), and charsets beyond the Unicode ones, ASCII and Latin-1 from the system charmaps
  (D-6.8). The formats are inspected and recorded as spec Appendix D before implementation;
  the decisions are D-4.18 (collation), D-5.4 (built-in `C.UTF-8` tables), D-6.8 and D-6.9
  (charsets, iconv), D-7.7 (loading), D-7.8 (locale objects, the current locale) and D-7.9
  (`rpmatch` with regex, phase 12). The Unicode 15.1.0 inputs are in `spec/unicode/15.1.0`
  (owner approval).
* **Delivered** (exit met, with the deviations listed):
  * **Unicode (IP-19, D-5.4):** `cargo xtask unicode-gen` builds `crates/core/src/unicode` from
    the committed Unicode 15.1.0 inputs: classes, simple and full case mapping, case folding,
    widths, canonical and compatibility decompositions, combining classes. They are the
    built-in `C.UTF-8` tables; `--check` is in the gate.
  * **Locales (IP-20, D-7.2, D-7.7, D-7.8):** compiled data only, as Appendix D records it:
    category files, the locale archive, name resolution with `locale.alias`, `LOCPATH`.
    `LC_CTYPE` (classes, maps, widths, transliteration), `LC_COLLATE` (D-4.18: `strxfrm` keys
    equal the reference library's, `strcoll` walks the elements without keys), and every
    `nl_langinfo` item of every category (D-7.10; the built-ins from `spec/abi/langinfo.tsv`
    through `cargo xtask locale-gen`). `setlocale` (composite names), `newlocale`,
    `duplocale`, `freelocale`, `uselocale`, `localeconv`, `nl_langinfo[_l]`, and the
    per-thread `__ctype_b_loc` family. Every locale-sensitive function of §4, §5 and §6 takes
    the thread's current locale.
  * **Charsets (D-6.8):** the system charmaps, read on first use: a safe gzip inflater, a
    parser, decoding and encoding by ranges; names and aliases matched like codeset names, and
    found by a directory scan when no file has the name. 200 charsets agree with the
    reference iconv in both directions over every mapping of the small charsets and a sample of
    the large ones; 5 differ by decision (recorded in D-6.8). A non-UTF-8 locale decodes and
    encodes through its codeset's charmap (`MbState` kind 5 holds an incomplete character).
  * **iconv (D-6.9):** `iconv_open`, `iconv`, `iconv_close`, with descriptors in heap words;
    the built-in Unicode codecs follow the reference library's observed rules (D-6.9);
    `//TRANSLIT` through the locale's table, or the UCD-derived one in the built-ins;
    `//IGNORE`; resumption at every split point.
  * **§8 radix (D-8.3):** the `strtod` and `wcstod` families take the radix from `LC_NUMERIC`
    (the `_l` forms from their object), including a multibyte decimal point.
  * **Tests:**
    * Differential tests against the reference library, each one C program built against the
      host glibc and against the products, compared line by line:
      * `locale_products.rs`: 11 locales (C, POSIX, C.UTF-8, en_US, de_DE, tr_TR,
        fr_FR.ISO-8859-1, ja_JP.EUC-JP, ru_RU.KOI8-R, zh_CN.GB18030, ps_AF); every
        `nl_langinfo` item, `localeconv`, the byte tables, wide classes and maps, the multibyte
        functions one call and one byte at a time, collation keys and order, the radix, the
        locale objects. Three lines differ by decision and are recorded.
      * `iconv_products.rs`: 279 conversions (every built-in codec, BOMs, errors, charmaps
        and their aliases, `//TRANSLIT`/`//IGNORE` in four locales, each also fed one byte at a
        time). Four lines differ by decision and are recorded.
      * `charmap_data.rs`: every installed charmap inflates, parses, and agrees with the
        reference iconv; the alias scan sees what the full parse sees.
      * `locale_data.rs`: the data formats against oracles (`LC_CTYPE` tables, `strxfrm`,
        `wcsxfrm`).
    * Clause and symbol tests: §5 (7 of 7 clauses), §6 (37 of 37; §6.4 through `liella::iconv`
      with the built-in codecs), §7.1–7.4 (24 clauses, 13 symbols).
    * Full gate on the two-node VM: 5,184 tests over both passes, 0 failed; 35 of 35 conformance programs;
      clippy, purity, unsafe-audit, csys-audit and the generators' checks (csys, abi, unicode,
      locale) clean.
  * **Budgets and measurements:** `bench/c/cbench_locale.c` builds against the products and
    the host glibc alike; five sanity budgets (the phase has no owner budgets). Results:
    `benchmarks/progress.md` entry 5. Profiling found the charmap encoder searching every
    range and stalling on a byte-wise store; merged ranges, a word-wise sequence and an ASCII
    fast path took the charmap conversion from 27× to 4.1× the reference library.
  * **Found and fixed:**
    * `setlocale(cat, …)` returned the built-in `"C"` name without a NUL after it (a slice of
      a plain literal), so C read past it. Every name a locale holds is now followed by a NUL
      (D-7.7); `__names` had the same exposure.
    * Raw NUL bytes inside four Rust literals in three files (`text.rs`, `data.rs`, an `s08`
      test), from earlier edits through a shell; they are `\0` escapes now.
    * From the VM's 16 cores and two nodes (phase-6 code): AMD reports no L4 cache (size 0,
      D-24.7), the parked-cache heap test assumed one node, `s25`'s `shmdt` test raced with
      itself, `s22_sync` summed its TSS destructor values without synchronisation, and the
      parked-cache test could see a test-harness thread park or adopt a cache.
  * **Deviations from the plan as written:**
    1. **`rpmatch` (§7.6)** moves to phase 12, with the regular expressions (D-7.9).
    2. **Stateful charsets** (ISO-2022, UTF-7, stateful EBCDIC) are not supported:
       `iconv_open` reports `EINVAL` (D-6.8).
    3. **Built-in transliteration** is derived from the UCD, not the reference library's
       table; the cases tested agree (D-6.9).
    4. **Grouping** in `strtod` is not in the spec; it arrives with printf's `'` flag in
       phase 8 (D-8.3).
    5. **Pending clauses:** `strfmon` (§7.5, phase 8), gettext and catgets (§7.7–7.8, phase 13).
  * **Multi-core and NUMA (§2.6 register):** locale data and charmaps are shared read-only by
    every thread; one lock serializes first loads, interning and changes of the global locale;
    every lookup is lock-free (register row 7). The per-thread `uselocale` choice, the ctype
    table pointers and `struct lconv` live in the thread block (D-7.8).
