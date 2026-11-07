# TVA-CONFORMANCE-ONCE: a conformance program runs once per products build, not once per symbol test

Id: TVA-CONFORMANCE-ONCE
Kind: code
Priority: 65
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 70 tool calls / 140k

**No owner ruling needed (CONSOLIDATE): every program still runs on the same products; only exact repeats are
skipped. If measurement shows a repeat can differ (see Isolation), stop and raise RULING NEEDED instead.**

## Why
TEST-VALUE-AUDIT (handoff/test-value-audit.md) found that the symbol suites call `conformance/run.sh <names>`
per test, so the same program runs many times per pass and again in pass 2, and once more in `cargo xtask
test`'s own conformance step (xtask/src/main.rs:290): s22_threads_symbols runs `s22_symbols` in 102 of its 201
tests, `s22_sync` 41, `s22_more` 37; s12_stdio_symbols has 110 calls, s01_startup_symbols 36 (19 x
`s01_symbols_options`), s18_processes_symbols 14; s00_conventions_clauses and s07/s11 symbol suites have their
own `conformance()` helpers. All `_symbols` suites cost 933 test-s per train. Estimated saving <= -0.6 min per
train. NOT VERIFIED: first measure the conformance share (time the run.sh calls of one suite).

## The change
1. Measure: per suite, the seconds spent inside conformance calls, and the number of distinct (program set)
   calls. Stop if the share is under ~150 test-s per train (report it; not worth the complexity).
2. One shared helper (support/, used by every `conformance()` copy) records a pass per program under
   conformance/build/ keyed by the program's built bytes, the products' hash run.sh already computes
   (conformance/run.sh `.products.key`), and the environment run.sh passes on. A later call for the same key
   skips that program; a failure records nothing. Reuse run.sh's own stamps if it can report per-program
   verdicts; do not add a second products hash.
3. Isolation: each program runs in its own process, so the calling test's process state cannot reach it; check
   that run.sh does not pass the test process's environment, rlimits, signal dispositions or umask into the
   program (if it does, those go into the key or the call is not skipped). Programs that test process-wide
   state of their own (signals, rlimits, locale, fds) are unaffected: they still run once in a fresh process.

## Proof (every plant still caught)
Plants with `cargo xtask plants`, each with `msg:`: a fault in a function `s22_symbols` checks (e.g.
pthread_once's return value): caught by the first symbol test that runs the program and by the conformance step;
a key that omits the products hash: caught by a test that changes the products key and expects a rerun;
recording a failed run: caught by a test that fails a program and expects no record. Also: a fresh run of
s22_threads_symbols runs `s22_symbols` exactly once (count from run.sh's output).

## JOBS
gate (direct tier, `--base origin/next`), plants, one timing run of the symbol suites before/after. Name each
label on `JOBS:`.

## Hand-back first line
`COMPLETE: conformance programs run once per products build; symbol suites -<n> test-s, plants <k>/<k> CAUGHT`.
