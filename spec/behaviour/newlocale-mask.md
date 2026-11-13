# newlocale: the category mask, `""` and a NULL name (§7.3)

Spec §7.3 (s7_3_c_1, s7_3_f_0) says `newlocale(mask, name, base)` takes the categories in
`mask` from `name` and fails with `EINVAL` for a bad mask, but not which masks are bad: the C++
library calls `newlocale(1 << LC_ALL, ...)`, a mask with only the bit numbered `LC_ALL` (6), which
names no category. Settled by probe (NEWLOCALE-LC-ALL-BIT).

Probe: `crates/core/tests/oracle/newlocale_mask.c`. Recorded output: `newlocale-mask.glibc.txt`
(the reference built with `cc -std=gnu17 -O1`, run with the installed locales, no `LOCPATH`:
first with `LANG=en_US.UTF-8 LC_NUMERIC=C.UTF-8` and no argument, then with the argument `env`
in four environments: `LC_ALL=C.UTF-8 LANG=en_US.UTF-8`; `LC_ALL= LC_CTYPE=C.UTF-8
LANG=en_US.UTF-8`; `LC_CTYPE=xx_YY.NOPE LANG=en_US.UTF-8`; none). Each accepted call prints the
codeset, the radix character and each category's name (`_NL_LOCALE_NAME`), in category order
skipping `LC_ALL`. The differential test `locale_products.rs::newlocale_masks_equal_the_reference`
runs the same five with `LOCPATH` at the test locales.

| Call | Reference |
|---|---|
| `1 << n`, n = 0..5, 7..12, any name, base NULL or an object | that one category from the name, the rest from base (C for NULL) |
| `1 << LC_ALL` (0x40) alone | every category from the name: `"C"`, `"POSIX"` (reported `C`), `"C.UTF-8"`, `"en_US.UTF-8"`, a composite name (each category its own), with base NULL or an object |
| `1 << LC_ALL` with `""` | each category from the environment as `setlocale(cat, "")` takes it (`LC_ALL`, `LC_<cat>`, `LANG`; an empty `LC_ALL` is ignored); none set gives `C` |
| `LC_ALL_MASK` (0x1fbf) | every category (same results as `1 << LC_ALL`) |
| `0` | the base unchanged (C for NULL) |
| `1 << LC_ALL` with any other bit (0x41, 0x46, 0x1fff, 0x2040) | NULL, `EINVAL` |
| `1 << 13`, `1 << 31` | NULL, `EINVAL` |
| a NULL name (masks 0x1, 0x40, 0x1fbf) | NULL, `EINVAL` |
| an unknown name, base NULL | NULL, `ENOENT` |
| an unknown name loaded into a base; `""` with an environment naming an unknown locale | NULL; `errno` seen 0 in some of these rows |

Liella matches every row but the last, where it sets `ENOENT` (s7_3_f_0); the oracle test
accepts exactly that `errno` and nothing else (decision record of NEWLOCALE-LC-ALL-BIT).
