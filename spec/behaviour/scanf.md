# scanf (item 8)

**Spec:** §12.7 (`scanf`, `fscanf`, `sscanf`, the `v` forms, the wide forms, the `__isoc99_` and
`__isoc23_` sets). Public interface: `<stdio.h>`, `<wchar.h>`.

**Clauses:** `s12_7_c_0`, `s12_7_c_1`, `s12_7_c_2`, `s12_7_c_3`, `s12_7_g_0`, `s12_7_f_0`; no clause
ID for the `__isoc99_` and `__isoc23_` symbol sets (§12.7; stated only in a ⚑ COMPAT, ⚑ PATH note).

**Probe:** `crates/core/tests/oracle/scanf_diff.c`. **Record:** `scanf.glibc.txt.gz` (gzip;
about 40,000 lines).

## What the probe does

The three symbol sets are called by name: `gnu` (the plain symbols), `c99` (`__isoc99_`) and
`c23` (`__isoc23_`). Each case runs one format against one input four ways:

* `s`: `sscanf` on the input string;
* `f`: `fscanf` on a temporary file holding the input, then `pos` (`ftell`), `next` (the next
  byte, `fgetc`) and `eof` (`feof`);
* `ws`: `swscanf` on the input converted with `mbstowcs` (skipped when it does not convert);
* `wf`: `fwscanf` on the temporary file, then `pos` and `next` (`fgetwc`).

A line shows the way, the set, the input, the format, the result, and the bytes each of the
four arguments received (`a=` to `d=`, up to the last byte changed; the arguments were filled
with `0xAA` before the call). For allocating conversions (`m`, and `a` in the plain set), `m0=`
and `m1=` show the allocated text, `null` when the pointer was set to NULL, or `unset` when it
was not written.

Sections: integer conversions over integer inputs; combined formats (literals, `%%`, `%n`,
positional arguments, suppression) over short inputs; allocations; a `streams` line of
successive scans on one stream and a `v`-form call; then, per locale (`C`, `en_US.UTF-8`,
`de_DE.UTF-8`), floating conversions, floating conversions with widths, and text conversions
(`%s %c %[...] %ls %lc %S %C`). In `C`, a `== partial` section covers inputs where the field is
only the start of a number.

The `partial` section and integers out of range are recorded differences in Liella (the
spec's §12.7 guarantee for partial fields; decision D-12.6 for out-of-range integers), listed
in `scanf_products.rs`.
