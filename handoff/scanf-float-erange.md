# SCANF-FLOAT-ERANGE: scanf floating conversions report ERANGE

State: branch task/scanf-float-erange, code commit 1424c78 plus a record fix (one-line body) on top; head = the
commit adding this note. Decision in force: D-12.26 (amends D-12.13). Host keke, role n1.

## What changed
* `crates/core/src/scanf.rs`: `Scanned.range` is set when a floating conversion that completes (also `%*f`) has
  a float-engine range result other than Ok (overflow, or tiny and inexact). Values and counts are unchanged.
* `crates/boundary/src/scanf.rs`: sets errno to ERANGE when `range` is set. A read or encoding error found in the
  same call takes precedence.
* Tests: `scanf_engine.rs::floating_range_errors_are_reported` (s12_7_c_3). `oracle/scanf_diff.c` prints errno in
  the floating grids, with new range inputs, so `scanf_products` compares errno with the reference for s, f, ws and
  wf in every size.
* `docs/differences-from-glibc.md` row "scanf floating range error, then end of input" (D-12.26).

## Probe-settled behaviour (keke, black-box; probes in the scratch work dir, erange.c and erange2.c)
* Before the change, Liella never set ERANGE. Values and counts were identical to the reference in all 145 lines.
* The reference sets ERANGE on overflow and on tiny inexact results, in every size and for `%*f`, sscanf, vsscanf,
  fscanf and swscanf. An exact subnormal and the smallest normal value leave errno unchanged, and so does inf.
* The one exception is the recorded difference: when the floating field ends at end of input and a later directive
  (white space, a literal or a conversion) then reaches end of input, errno is back to its value on entry.
  `%lf%n` and a later field that is not at end of input keep ERANGE.
* INFORMATIONAL (out of scope, untouched): with empty input, `%lf` returns EOF and errno is 0 in the reference
  whatever errno was on entry. Liella leaves errno unchanged.

## Results
* plants: scanf-erange-overflow CAUGHT. It ran on 1424c78; the code is identical at head.
* gate 1424c78: 1976 tests passed, 0 failed; scanf_products and conformance were ok. Only `xtask static` failed,
  on the record's newline, fixed after. lint2 on the final tree: clippy, static and records ok.
  gate2 (final tree) was started detached.

## Open
* RULING NEEDED: there is no compat option for the difference. The recommendation is none; the reason is in the row.

Next step: none for this story; land after gate2 is green.
