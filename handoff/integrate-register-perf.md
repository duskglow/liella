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
