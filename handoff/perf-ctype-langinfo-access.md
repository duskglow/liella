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
