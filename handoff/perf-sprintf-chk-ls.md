# PERF-SPRINTF-CHK-LS (one page)

State: branch task/perf-sprintf-chk-ls, code commit b7e58c4a on origin/next 361836f7. Role keke:~/w/n1.

## Profile before the change (job perf-sprintf-chk-ls-trace0, wl_trace spr_ls sprintf, call 11)
TOTAL 862 instructions: boundary vsnprintf (simple_into parse + render_pair_s inlined) 355, ToBuffer::put 176
(4 out-of-line calls of ~44 for pieces of 1-10 bytes), simple_conv 100, printf_text 60, vsnprintf6 49,
simple_star 37, strlen 33, va_long 30, sprintf 22. glibc percall reference: 726 (spr_ls), 744 (chk_ls).
The surplus sits in the piece writes, not in padding byte by byte or a double strlen (the brief's two guesses).

## Change
`Out::room(n)` (core printf.rs, default None) lends the next n bytes when all fit; ToBuffer implements it
with put's own common-case test (end < max, end <= slen). render_pair_s computes the total first and, when it
fits an int and the sink lends the room, copies the pieces in place (mem::copy / mem::fill, inlined). Everything
else (cut snprintf, object too small, result over INT_MAX, other sinks) keeps the piecewise puts unchanged.
Expected gain: ~4 x 44 put calls -> one test plus inline small copies (~100 instructions).

## Tests
conformance/c/s12_printf.c, clause s12_6_c_r4 (run by core test s12_6_c_r4): two-string formats vs bytes
built by hand (widths -40..40 through `*`, fixed widths, left/right, text around/none, multibyte bytes, a
string longer than its field, precision cuts) via sprintf, __sprintf_chk (exact object), snprintf with room
and at every smaller bound (cut + guard byte); __sprintf_chk with objects of 1-14 bytes for a 14-byte result
aborts and writes nothing past the object (shared-memory buffer, C0.5).

## Jobs (keke)
- perf-sprintf-chk-ls-lint: clippy --workspace --all-targets --all-features -D warnings + xtask static: rc 0.
- perf-sprintf-chk-ls-plants (cargo xtask plants, ~/work/PERF-SPRINTF-CHK-LS/plants.txt, plants-1): rc 0;
  6 WARN-NO-MSG (room-max, room-slen, room-pos, place-left, place-after-fill, place-advance: each rc 1 with
  "conformance: 0 passed, 1 failed", baseline green), 1 EQUIVALENT (fits-int: only a >INT_MAX result tells the
  paths apart). The WARN cause: `affected --only conformance:<prog>` does not echo the program's FAIL lines, so
  `msg:FAIL s12_6_c_r4` cannot match; the msg should have been `conformance: 0 passed, 1 failed`.
- perf-sprintf-chk-ls-gate (affected --base origin/next --run), perf-sprintf-chk-ls-percall (wl_percall --base
  361836f7, sprintf_chk.c cases), perf-sprintf-chk-ls-counts1 (bench --compare --counters --runs 3 cbench_opt):
  run in that order by ~/work/PERF-SPRINTF-CHK-LS/chain.sh; results at hand-back time are in the hand-back.
- Superseded: perf-sprintf-chk-ls-counts-before / -counts0 (before-bench on origin/next; queued, never started,
  killed so the role could be synced). "Before" counts = the brief's parity-rows-1106-counts (origin/next) and
  the base column of -percall.

## Open / next step
- NOT VERIFIED at writing: the percall and counts1 numbers (the 1.05x target). If -percall shows chk_ls/spr_ls
  above ~760, the remaining surplus is the per-call parse (simple_into + simple_conv + simple_star, ~190):
  next idea is a one-entry plan cache keyed by the format address for the buffer path (ruling: costs a static).
- If any cbench_opt row is slower in -counts1, revert b7e58c4a (rule: no measured row slower).
- Saturation register row 3 (docs/saturation/14-...) not updated with this run's numbers.
- Plants could be rerun with msg `conformance: 0 passed, 1 failed` to turn the WARN lines into CAUGHT.
