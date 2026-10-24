# Plants template for a libm fast path (Track M milestone 6, M6-0)

Every fast-path story (M6-1 onwards) runs these five plants on its own code with its direct tests, one
plant per line in the `tools/plant.sh` form, each with a `msg:<regex>` naming the failing test. All must
be CAUGHT; an EQUIVALENT one needs a written reason. Replace `<file>`, `<fn>`, the patterns and the test.

| # | Plant | What it proves |
|---|---|---|
| 1 | the error bound ε halved (`eps * 0.5`) | the proved bound is not loose by 2x unnoticed: the accuracy suite finds a misrounding |
| 2 | the rounding test inverted (`if !same` -> `if same`) | the fast path's result is checked, not trusted |
| 3 | the rounding test disabled (always take the fast path) | the fallback is needed: the suite finds the operands it fixes |
| 4 | one table entry or polynomial coefficient perturbed by 1 ulp | the table and coefficients are covered |
| 5 | the reduction constant shortened (for example `ln2_lo` set to 0) | the argument reduction's extra precision is needed |

Template lines (fields: file, perl substitution anchored to the function, test command, expectation,
message):

```
crates/math/src/<file>.rs	perl:s/(fn <fn>_fast.*?)eps \* 1\.0/$1eps * 0.5/s	cmd:cargo test -p liella-math --test elem_products	caught	msg:<test name>
crates/math/src/<file>.rs	perl:s/(fn <fn>_fast.*?)if !same/$1if same/s	cmd:cargo test -p liella-math --test elem_products	caught	msg:<test name>
crates/math/src/<file>.rs	perl:s/(fn <fn>_fast.*?)if !same \{/$1if false {/s	cmd:cargo test -p liella-math --test elem_products	caught	msg:<test name>
crates/math/src/<file>.rs	perl:s/(<TABLE>: .*?0x[0-9a-f]+?)([0-9a-f]),/$1 . sprintf("%x", hex($2) ^ 1) . ","/se	cmd:cargo test -p liella-math --test elem_products	caught	msg:<test name>
crates/math/src/<file>.rs	perl:s/(const <REDUCTION_LO>: f64 = )[^;]+;/${1}0.0;/	cmd:cargo test -p liella-math --test elem_products	caught	msg:<test name>
```

The fallback-rate check uses `liella_math::stats` (feature `fastpath-stats`, on in the math crate's
tests): `let before = stats::counts("exp");` ... `stats::counts("exp").since(before).check_fallback("exp", 10_000)`.
The speed check uses the `cbench_libm` rows: `cargo xtask bench --compare cbench_libm` (Liella, glibc
dynamic, glibc static) on an idle host.
