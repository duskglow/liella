# INTEGRATE-REGISTER-PERF handoff

State: branch task/integrate-register-perf (stacks on INTEGRATE-REGISTER-MATH). Role kanon:~/w/kn1.

Per story (code taken by cherry-pick of the story's own commits; register rows moved into the section file they belong to):

| Story | Result | Where |
|---|---|---|
| PERF-STRSTR-LONG | PORTED | `scan.S` 128/64-byte scan groups, test in `strstr_find.rs`; row 3 in section 15 |
| PERF-CTYPE-LOC | PORTED | `locale.rs` tail-call fill; row 7 in section 15 |
| PERF-WCTYPE-UTF8-REST-3 | PORTED | `view.rs` out-of-line file-backed arms; rows in section 15 (the earlier split/by-reference attempts were reverted on the branch, so net zero, not ported) |
| PERF-MEMSET-4K-SSE2-2 | PORTED | `scan.S`, `hot.rs`, `conv.rs`, `ffi.rs`, `liella_csys.h`; result block 14.1a appended to section 14 |
| PERF-GETC-PUTC-TRIMS-2 | CONTAINED | the code (getc_c tail call) is already on next; only row 9 text, handoffs and reports carried over (section 14) |
| BENCH-ROWS-UNMEASURED | PORTED | `bench/c/cbench_unmeasured.c`, budgets, instr limits; its register text is new section 17 (13 was taken) |
| PERF-FNMATCH-PATHNAME-3 | PORTED | the four story commits on `fnmatch.rs` (literal run after an ordinary step, last-`]` memo, tests); the branch's other 39 files were historic-next drift and were not taken; note in section 14 |

Not re-measured: no counts were re-measured on next. The recorded counters are from the historic branches (their own hosts). The code
under them applied without conflicts, but strstr/memset/ctype/wctype/fnmatch rows should be re-counted
(`cargo xtask bench --compare --counters`) by whoever next touches those rows. Lab host names `sumire` in the moved text were changed to `mei`.

Plants: fnmatch direct-matcher plants only (run-loop stops, last-`]` memo and bound); see the plants job. The assembly and
ctype/wctype changes have no plants here (perf-only changes, covered by their historic plants, not re-run).

Next step: dispatcher reads the gate and plants jobs; re-count the rows above when convenient.

## INTEGRATE-REGISTER-PERF-2 (continuation, branch task/integrate-register-perf-2, role mei:~/w/s1)

Predecessor merged (one conflict: the register index, both section 16/17 lines kept). Plants failure: the three fnmatch survivors were
- run-backslash-stop: CAUGHT now (`a\*` against the string `a\*` must not match; added to direct_brackets_after_a_first_close_and_runs_stop_at_metacharacters);
- no-close-memo (`.unwrap_or(0)` -> `usize::MAX`): CAUGHT by the new `the_last_close_memo_is_filled_once_and_kept` (the memo is invisible in match results, so the test reads close_before and its memo directly); an extra plant memo-never-kept is also CAUGHT;
- close-bound (`<= pi` -> `< pi`): EQUIVALENT. `p[pi]` is `[`, so the last `]` is never at `pi`; the only value that could differ is the "no `]`" 0 at pi = 0, which then goes to bracket_has, which reports no valid bracket and the `[` is a literal: same result. Job integrate-register-perf-2-plants prints CAUGHT/EQUIVALENT per plant.
Also: comments "register 13.5" -> 16.5 (fast64.rs, tables.rs); register section 16 rows 2 and 5 have DONE notes (no separate counts: those savings were counted together with neighbouring rows).
Counts: strstr and memset re-measured (section 15 appendix). ctype/wctype/fnmatch/libm counts: job integrate-register-perf-2-counts (output ~/work/INTEGRATE-REGISTER-PERF-2/counts-*.txt on mei); their register rows are NOT yet updated.
Not done: re-plants of the asm (scan.S), ctype, wctype and memset ports (budget). Gate: affected escalated to FULL, rc 0. clippy --workspace --all-targets and cargo xtask static: ok (job -lint).
Next step: read counts-*.txt, update the ctype/wctype/fnmatch rows in section 15/14, plant the four non-fnmatch ports.
