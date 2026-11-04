# PERF-CTYPE-LANGINFO-ACCESS handoff (NOT READY: checkpoint, no job ran)

Branch task/perf-ctype-langinfo-access. Code commit: 94ea6a7 (crates/boundary/src/locale.rs only). Host keke, role n4.

## Done
- Disassembled the five accessors from a built libc.so.6 (Liella's own code). The fast paths are already short.
  - isalpha: 12 instrs filled path (fs load, test, range cmp, `add -256; je` from mapped_bytes rebasing, mov, load, shr, and).
  - __ctype_b_loc: 9 instrs with push/pop because the cold fill was a call, not a tail call.
  - nl_langinfo: view (1 fs load) then ~25 instrs of category-table decode (kind check, bound check, two loads, two tests on the enum Item).
  - wcwidth: heavy prologue (push r14/rbx, sub rsp) before the stage tables.
- Edit 1: ctype_slot tail-calls ctype_fill(slot) (fill returns the slot address), so the filled path has no push/pop. Expected __ctype_b_loc 14 -> 12 instr/call.
- Edit 2: byte_class indexes from the stored word (w + 2c) after one range test, dropping the rebase and null check. Expected isalpha -2 instr.
- clippy and xtask static passed on this head (job pcla-lint, rc 0, on the head before the handoff files).

## NOT VERIFIED
- No gate, plants or percall result: jobs perf-ctype-langinfo-access-gate / -percall-a / -plants sat in the keke slot queue for
  ~10 min and died when my foreground ssh calls were stopped (jobrun blocks the ssh; use run_in_background with a long timeout or nohup).
- No before/after counts. Rows nl_langinfo(_l) and wcwidth not changed; parity not reached for isalpha (est. ~20 vs 16) or __ctype_b_loc (est. 12 vs 10).

## Next step
1. Re-run: gate (`cargo xtask affected --base origin/next --run`), plants (script was ~/work/pcla/plants.sh on keke: fill-skipped, range-test-dropped, wrong-slot-offset on locale.rs), percall (validation/percall/norow.c ctype_b_loc; locale.c wcwidth_2byte, toupper_isalpha; add a nl_langinfo case).
2. nl_langinfo: give LocaleView a flat per-category array of resolved usize results (word value or address) built with the view, so langinfo is: bounds check + one load. Needs core view.rs change.
3. isalpha: return the masked class (as an int, nonzero) instead of ret::truth to drop the shr; check tests that expect exactly 1.
4. wcwidth: move the narrow Latin/ASCII answer before the frame setup (split the cold path out).
5. Record unreached rows in docs/saturation/ with numbers (no lab host names outside handoff/).
