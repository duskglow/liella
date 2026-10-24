# strfmon (item 3)

**Spec:** §7.5 (`strfmon`, `strfmon_l`, `__strfmon_l`); `LC_MONETARY` items (§7.4,
`localeconv`). Public interface: `<monetary.h>`.

**Clauses:** `s7_4_c_1`, `s7_5_c_0`, `s7_5_f_0`.

**Probe:** `crates/core/tests/oracle/monetary_diff.c`. **Record:** `monetary.glibc.txt`.

## What the probe does

For each locale (`C`, `en_US.UTF-8`, `de_DE.UTF-8`, `fr_FR.UTF-8`, `ja_JP.UTF-8`,
`it_IT.UTF-8`, `de_CH.UTF-8`, `nl_NL.UTF-8`, `en_IN.UTF-8`; each section starts `== <locale>`),
every format below is applied to ten values with `max` 200. A line shows the format, the value
×1000, `max`, the result, `errno` on failure, and the text including the terminator.

Formats: `%n %i %=*#10n %^n %+n %(n %!n %-14n %14n %#5n %#5.0n %.3i %=0#8n %(#5i %^#9.1n
%!(#7n %+#6i %-#6.2n| "a %n b %% c" %=x-16.1i|` and the malformed `%q %#n %.n %= %+(n %(+n %L`.
Values: 0, 1234.567, -1234.567, 1234567.891, -0.0, 0.005, 1e20, -5, 999.995, 12.

Then `%n` of -1234.567 with `max` 0 to 13 (where the result stops fitting), a `long double`
pair (`%Ln|%Li`), three amounts in one format, and `strfmon_l` with a `de_DE.UTF-8` locale
object while the current locale is the section's.

The `LC_MONETARY` values of each locale can be read with `localeconv()` or `locale -k
LC_MONETARY` under the same `LOCPATH`.
