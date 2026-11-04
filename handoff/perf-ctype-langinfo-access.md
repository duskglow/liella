# PERF-CTYPE-LANGINFO-ACCESS handoff (-3 run)

Branch task/perf-ctype-langinfo-access-3. Code commit 94ea6a7 (crates/boundary/src/locale.rs): ctype_slot tail-calls
ctype_fill; byte_class indexes from the stored word with one range test.

## -3 run
- The -2 plants job: range-test-dropped SURVIVED. Added test byte_class_out_of_range_is_false
  (crates/boundary/tests/s05_ctype_symbols.rs): all 12 class functions on values past both table ends, a sweep and the extreme ints.
- Jobs: pcla3-plants-b (3 plants, fresh RAM copy), pcla3-gate-b (clippy, static, affected --run; not yet read). Results in the hand-back / report json.

## Next step
Not done (percall counts, nl_langinfo flat per-category array, isalpha masked class, wcwidth early narrow answer,
saturation-register notes): run validation/percall before/after, then those items.

## -4 run
- Cause of pcla3-plants-b rc=3: the new test's table was typed `fn(c_int)` but held `extern "C"` fn items (E0308). Fixed.
- Second cause (found by pcla4-plants, range-test-dropped SURVIVED): the test thread had no stored ctype word, so the
  answer came from the locale (`class_unstored`), never the range test. The test now calls `__ctype_b_loc()` first.
- pcla4-plants-b (fresh RAM copy plants-5): fill-skipped CAUGHT, range-test-dropped CAUGHT, wrong-slot-offset CAUGHT. rc 0.
- Gate: pcla4-gate-b (affected --run). Superseded: pcla4-plants (range-test-dropped SURVIVED, before the fix).
- Still not done (no budget): percall before/after counts, nl_langinfo flat per-category array, isalpha masked class,
  wcwidth early narrow answer, saturation-register notes.
