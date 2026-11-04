# FUZZ-TIMEOUTS-RECHECK

Branch task/fuzz-timeouts-recheck. Re-ran V-3's four fuzz time-outs (keke ~/work/pre14-fuzz-campaign/run-20261023T065839Z-b/artifacts)
with `cargo xtask fuzz <target> --repro` on next (campaign limits: regex/regex_dfa/glob 10 s, locale 20 s).

| Input | Result on next |
|---|---|
| regex timeout-af9dfee4 | 9 ms, fixed earlier (regex time-out stories) |
| regex_dfa timeout-d8d6be50 / slow-unit-d977c8d7 | 0.47 s / 0.64 s, fixed earlier |
| locale timeout-bf6ed537 | still timed out; cause: a locale d_t_fmt with a ~60 digit width made the composite buffer (`Buf::field`) push padding one unit at a time until memory ran out. Fixed: width refused with ENOMEM up front (`COMPOSITE_MAX` = 2^20 units); now 1 ms. Unit test `composite_field_width_is_bounded_before_padding`; plants (2) caught; input pinned in fuzz/seeds/locale. |
| glob timeout-5351ade2 | still times out (> 10 s). NOT fixed here, see below. |

## glob: cause and fix-story brief (not done: needs a ruling on the bound)
The input has 6 `{` (harness allows 8), two groups with ~45 alternatives each (`{a,,,,...,*}`), plus several two-way groups. The
number of expansions is the product of the alternatives (order 10^5 or more from a 4 KiB pattern), and each expansion is walked
against the tree, so one call runs past 10 s. The harness bounds only the number of `{`, not the product. The product itself is
produced by the engine (`braces` in crates/core/src/pattern/glob.rs), so it is an engine question, not only a harness one.
Proposed story GLOB-BRACE-EXPANSION-BOUND: decide a bound on expanded alternatives (fail with GLOB_NOSPACE) or a work budget like
the regex one; if the engine keeps unbounded expansion for glibc compatibility, bound it in the harness (product of alternative
counts) and record a differences row + COMPAT id if the engine changes. Not measured exactly (no count of expansions taken).
See SECURITY-PRIVATE in the hand-back; docs/fuzzing.md only says "open".

## Checks
gate (affected --run) rc 0, clippy -D warnings rc 0, cargo xtask static rc 0, plants 2/2 caught. Jobs on shiki:
fuzz-timeouts-recheck-gate, fuzz-timeouts-recheck-plants (the -repro job fuzz-timeouts-recheck-repro ran on the head before the code change; superseded).
