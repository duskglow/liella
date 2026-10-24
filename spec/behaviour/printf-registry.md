# The printf registry (item 1)

**Spec:** §12.11 (`register_printf_specifier`, `register_printf_function`,
`register_printf_modifier`, `register_printf_type`, `parse_printf_format`, `printf_size`,
`printf_size_info`, `__printf_fp`); §12.6 for the format language they extend. Public interface:
`<printf.h>` (`struct printf_info`, the `PA_*` constants, the function types).

**Clauses:** `s12_6_c_p1`, `s12_6_c_2`, `s12_6_c_3`, `s12_6_c_4`, `s12_6_c_5`, `s12_6_c_6`,
`s12_6_g_3_1`, `s12_6_f_0`, `s12_11_c_1`, `s12_11_c_2`, `s12_11_c_3`, `s12_11_c_4`, `s12_11_t_0`.

**Probe:** `crates/core/tests/oracle/registry_diff.c`. **Record:** `registry.glibc.txt`.

## What the probe registers

| Letter | Handler | `arginfo` |
|---|---|---|
| `Q` | prints the `printf_info` fields it receives and its argument, writes `<Q` *n* `>` | one `PA_INT` |
| `V` | writes `<V` *string* *double×100*`/100` *long*`>` | three: `PA_STRING`, `PA_DOUBLE`, `PA_INT\|PA_FLAG_LONG` |
| `d` | writes `D(`*n*`)`; returns -2 for a negative value | one `PA_INT` |
| `E` | sets `errno` to `ERANGE`, returns -1 | as `Q` |
| `N` | writes `<N>` | returns -1 |
| `P` | writes `(`*a*`,`*b*`)` from a two-int struct | one argument of the type `register_printf_type` returned, size 8 |
| `k`, `K` | `printf_size` | `printf_size_info` (registered with `register_printf_function`) |

Handlers write with `fputs` only. Modifiers registered: `XYZ`, `XY`, `X`, then `m` followed by a
letter until registration fails.

## Sections of the record

* `== registration`: return values and `errno` of each registration, including the invalid ones
  (letter -1 and 256, an empty modifier, a modifier containing a character above 255), the
  number `register_printf_type` returned, the bit each modifier received, and the result once
  the bits run out.
* `== parse_printf_format`: for each format, the count returned and the types written (an
  unwritten slot shows -7), how many times an `arginfo` function was called, and the result
  with room for one type. The built-in `%d` is registered over, so built-in integer types are
  probed with `%i`, `%u` and the other letters.
* `== conversions`: each `Q handler` line is one handler call, showing the `printf_info` it
  received (`prec`, `width`, `spec`, the length flags, `alt`, `space`, `left`, `showsign`,
  `group`, `extra`, `wide`, `i18n`, `user`, `pad`) and its argument; each result line shows the
  function's return value and the text produced. Covered: flags, width and precision (fixed and
  `*`), every length modifier, the registered modifiers (longest match, and what follows a
  match), several arguments for one conversion, a registered letter that replaces a built-in
  one and declines, a failing handler, an `arginfo` function that declines, a registered type,
  truncation by `snprintf`, `printf` to `stdout`, `swprintf`, `asprintf`.
* `== printf_size`: `printf_size` through `%k` (1024-based) and `%K` (1000-based) with flags,
  width and precision, over twelve values (`value#` indexes 0, 1, 999, 1000, 1023, 1024, 1536,
  1048576, 1e9, 1e30, -2048, 1e300), the infinities and a NaN, and `printf_size_info`'s result
  and type for `double` and `long double`.
* `== __printf_fp`: `__printf_fp` called directly on a memory stream for `f e E g G a A`, six
  values (`v` is the value×1000) and six `printf_info` variants: 0 none; 1 `prec` 2; 2 `prec` 0
  with `alt`; 3 `width` 14, `group`, `pad` `'0'`; 4 `width` 12, `left`; 5 `showsign`. Then one
  `long double`.

## Observations the record shows directly

* The registration functions return 0 on success; an invalid letter gives -1 with `EINVAL`.
* Modifier bits are 1, 2, 4, … in registration order; after sixteen, registration returns -1
  with `ENOSPC`.
* The first added type is numbered 8.
* A handler's output is the whole field: `[%5Q]` gives `[<Q2>]`; width, precision and flags
  reach the handler only through `printf_info`.
* A handler returning -2 is replaced by the built-in conversion (`%d` of -3, -4).
* A handler returning -1 makes the call fail with the `errno` it set.
