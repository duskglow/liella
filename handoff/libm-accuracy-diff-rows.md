# LIBM-ACCURACY-DIFF-ROWS

Branch task/libm-accuracy-diff-rows. Docs only: docs/differences-from-glibc.md, section Mathematics.

* Accuracy principle paragraph at the top (rulings #2760/#2761).
* "No compatibility option (owner 2026-11-05)" added to the Decision cell of the five correctly-rounded rows (f32/f64 elementary, long double elementary, special f32/f64, special long double, complex).
* New measured-agreement list (from libm-slow-path-cause report section 2) and a new _Float128 row.
* No test matches the changed wording; the static trace check failed on a first table form (rows without clause ID) and passed once the numbers became a list.
* Gate: libm-accuracy-diff-rows-gate-2 (static, clippy -D warnings, affected) rc 0.
* No fast tier added, so no other row changes.
