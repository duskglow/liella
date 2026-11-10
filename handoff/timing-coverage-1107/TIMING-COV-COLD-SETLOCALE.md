# TIMING-COV-COLD-SETLOCALE: the first setlocale(LC_ALL, en_US.UTF-8) of a process costs about 2.2x glibc

Id: TIMING-COV-COLD-SETLOCALE
Kind: code
Priority: 70
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 calls / 160k
Stacks-on: none

## The change
Cut the cold first `setlocale(LC_ALL, "en_US.UTF-8")` (and the second call and the cold second
bindtextdomain+gettext, measured in the same child) toward glibc's cost on both classes. Do not redo
COLDSTART-SETLOCALE-FIRST(-2) (landed; read its handoff first) or anything in docs/saturation/05-locale-ctype-time-gettext.md.

## Rows (1106E, time IVB / N100; cbench_opt cold rows)
first_setlocale_en_US 2.174 / 2.171; second_setlocale_en_US 1.717 / 1.545; second_bindtextdomain_gettext
1.567 / 1.626. No instruction column (cold rows).

## Evidence
- Build host (Skylake-SP class, mei, TIMING-COVERAGE-1107 counters-mei.txt; the bench's own medians of 31 children):
  first_setlocale 110.0 us Liella vs 45.8 us glibc; second_setlocale 175-186 ns vs 151 ns; second_bindtextdomain
  468-473 ns vs 368-376 ns.
- docs/saturation/05-locale-ctype-time-gettext.md (first setlocale row): "not at a limit: the first heap use (about
  14 faults, 7 to 9 syscalls) and archive pages hold most of the gap" (N100 probe deltas, 730f416b).
- validation/profiles.md: in the locale-heavy short-program profile, `liella::locale::name::lookup_alias` is 11.7% of
  Liella's samples, kernel 57.9%.

## Hypothesis (to verify before changing anything)
The gap is kernel work: page faults on the first heap growth and on the archive pages touched by the name lookup.
Measure per phase (name/alias lookup, archive open+map, per-category setup, heap) with a static probe and
perf stat -e page-faults,instructions:u,instructions:k (-r 200) on Liella and glibc; then remove faults (fewer
touched archive pages, one up-front allocation) before touching instructions.

## Tests and plants
Existing setlocale/locale tests on the direct tier; add a test only for a new code path (name the spec clause).
Plants on each changed function. Do not add a wall-clock assertion; a fault-count bound may use the existing
hot_text_products style (startup/setlocale fault budgets).

## Rows to time
cbench_opt cold rows (`CBENCH_ONLY=cold`; `bench --rows` cannot select cold rows, escalation #3370): first_ and
second_setlocale_en_US, second_bindtextdomain_gettext, plus cold_exec_once and every other first_/second_ row (no
row may get slower).
## JOBS
Gate: `cargo xtask clippy`, `cargo xtask static`, then the direct tier (`cargo xtask affected --run --base origin/next`)
through jobrun on your role. Plants: `cargo xtask plants <file>` on every changed function (msg: regex per plant).
Counts before/after on base and head (region counters or the probe, both sides printed). Timing: book an Ivy Bridge
and an N100 host (`ops timing-book`), stage once with `cargo xtask bench --stage`, run the rows below base vs head
vs glibc interleaved (>= 3 rounds), cancel the bookings when done. Keep only a change that makes no measured row slower.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
