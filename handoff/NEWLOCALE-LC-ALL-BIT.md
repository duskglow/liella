# NEWLOCALE-LC-ALL-BIT: newlocale(1 << LC_ALL, ...) as the reference answers it

State: branch `task/newlocale-lc-all-bit` (from origin/next), code commit 5c5d05391; this note and
the report json in the commit after it. Role mei:~/w/s2.

## What changed

* `crates/boundary/src/locale.rs` `categories`: the mask `1 << LC_ALL` (bit 6) alone is every
  category; with any other bit, and bits 13 and up, it stays `EINVAL`.
* `newlocale(mask, "", base)`: each asked category from the environment, by the rule
  `setlocale(cat, "")` uses (`global_locale::env_name`; new `global_locale::newlocale_env`).
  Before, `""` was looked up as a name and failed with `ENOENT`: `std::locale("")` needs it.
* `newlocale_c`: a NULL name is `EINVAL` (it was `EFAULT`; the reference says `EINVAL`).
* Decision: pending record `records/pending/NEWLOCALE-LC-ALL-BIT-1.toml` (`D-NEW`, series D-7):
  the coordinator assigns the id.

## Probe-settled behaviour (spec gap §7.3)

spec/behaviour/newlocale-mask.md (+ `.glibc.txt`, 172 lines). Bits 0..5, 7..12 alone: that
category; bit 6 alone: every category (any name, base NULL or object, composite names too); bit 6
with any other bit (0x41, 0x46, 0x1fff, 0x2040): EINVAL; bits 13, 31: EINVAL; mask 0: base
unchanged; NULL name: EINVAL; `""`: per category from LC_ALL / LC_<cat> / LANG.

## Proof

* `crates/core/tests/oracle/newlocale_mask.c` + `locale_products.rs::newlocale_masks_equal_the_reference`:
  the probe against the reference and Liella in five environments, line by line.
* Clause rows: `s7_3_c_1` (bit 6 with a base and with NULL), `s7_3_f_0` (five refused masks,
  NULL name).
* `conformance/c/s07_newlocale_cxx.c` (LINK-CXX), run by
  `s07_locale_symbols.rs::cxx_locales_through_the_lc_all_bit`: iostreams, `std::locale("C")`,
  `std::locale("")` (RUN-ENV `LANG=C.UTF-8 LC_NUMERIC=C`), `std::locale("C.UTF-8")`, imbued
  number output, the direct call, an unknown name throwing. Job `newlocale-lc-all-bit-cxx` on mei
  ran it three ways, all rc 0: the reference (host c++, dynamic), Liella static (the runner), and
  Liella dynamic (`libc.so.6` by path + Liella's `ld.so`, libstdc++/libgcc_eh linked statically).
  The harness's dynamic mode does not take `LINK-CXX` (it compiles with `$cc -std=c17`), and a
  fully dynamic C++ program (libstdc++.so.6 found by the loader) needs Liella's `ld.so` to search
  for libraries, which it does not yet (conformance/README.md): that case waits for the loader's
  library search; the static-libstdc++ dynamic run above is the part that can run today.
* Plants: `handoff/NEWLOCALE-LC-ALL-BIT-plants.txt` (6): refusing the bit again is caught by the
  C++ program (`bit-refused-cxx`) and by the clause test; bit 6 with other bits; `""` not from the
  environment; the environment rule ignoring the category; NULL name back to EFAULT.

## Siblings checked (brief step 4)

`categories` is the only place a mask is checked against the categories (no other use of
LC_ALL_MASK or a category-mask test in crates/*/src or csys). `__newlocale` shares
`newlocale_c`. The base merge goes through the same `categories` result. `duplocale`,
`uselocale`, `freelocale` take no mask. `setlocale` and `current_name` take a category
number, where 6 is LC_ALL and already handled.

## Open

* RULING NEEDED: where a load into a base (or `""` with an environment naming an unknown
  locale) fails, the reference was seen to leave errno 0; Liella sets ENOENT (spec s7_3_f_0).
  Recommend keeping ENOENT; the coordinator decides whether that needs a differences row (no
  compat flag: errno 0 on a failure tells a caller nothing). The oracle test accepts exactly that
  errno difference.
* Next step: land; then a loader story (library search in `ld.so`) lets the harness's dynamic
  mode take LINK-CXX programs.

Plan rows: D-7.3 (base reuse), D-7.8 (names), the new pending D-7 record.
