# DL-DLOPEN-LOAD (DL-16a): handoff (run 3, DL-DLOPEN-LOAD-3)

**State.** Branch `task/dl-dlopen-load-3` (runs 1-2 included; cut from `task/dl-dlopen-load-2`, whose base
DL-LIBC-START-10 is on next). Role shiki:~/w/sh1. dlopen and dlerror work end to end (kernel-started and
`ld.so PROGRAM`); the owed records exist as pending records; the branch can land once the jobs below are green and the
coordinator assigns the five pending ids.

**Run 3 commits.** `a4aa95747` the gate fix; `7566f2c83` records, differences rows, #5125 test, plants file;
`33b0cf366` `dlopen_rules.rs` and the static-program test; `bf1d41440` clippy (test module last); then this note and
`handoff/reports/DL-DLOPEN-LOAD-3.json`.

**Gate failure of run 2, found and fixed.** `a_loader_change_does_not_select_the_math_suites` was right:
`crates/boundary/src/dlerr.rs` is built in every boundary build (not feature-gated) and called
`liella_ld::dlopen::compose`, so liella-math's tests (boundary with `selftest`) really linked liella-ld. `compose`
moved into `dlerr.rs` (private, unit test `s27_1_c_5_compose_...`); the selector is unchanged. Rule for later DL
stories: an ungated boundary module must not name a liella-ld item.

**Records (pending; ids for the coordinator to assign, in this order).** `records/pending/DL-DLOPEN-LOAD-3-1..4.toml`
are COMPAT-NEW: -1 #5125 (no IFUNC resolver in a failing set; `decisions` D-27.31), -2 #5126 (`ld.so` by any path is the
start-time object; D-27.34), -3 #5294 `e_ehsize` != 64, -4 #5294 `p_align` not a power of two (both D-27.7; both
ignored in a secure process). `-5` is D-NEW series D-27: DL-16a's loader design (state at the first `dlopen`, check
order, mode refusal = reference behaviour so no row, texts, constructors after the borrow, identity, capacity 512).
**On assignment also update the references**: the four new rows at the end of the "Deliberate user-facing
incompatibilities" table of `docs/differences-from-glibc.md` cite `records/pending/DL-DLOPEN-LOAD-3-<k>.toml`, and
`-5`'s body says "this story's pending records" for the compat ids.

**Tests added.** `crates/ld/tests/dlopen.rs`: checks 25-29 (#5125: two roots, both dependency orders, fail with no
resolver run; control: the IFUNC object alone runs it) and `s27_1_f_0_static_program_dlopen_fails_with_text`
(libc.a). `crates/ld/tests/dlopen_rules.rs`: check order and the #5294 refusals on mutated copies of the test's own
executable, unknown-bit precedence, open-count saturation. Plants: `handoff/DL-DLOPEN-LOAD-3-plants.txt` (7).

**Still owed / open.** CONF-DYN-HELPERS: flip its pending dlopen programs once it is on next. INFORMATIONAL for
the coordinator: the process capacity of 512 objects (one more is refused with a text) is a limit the reference
was not probed for; DL-16b or a later story should probe it and decide (row or larger arena).

**Known gaps (later stories, unchanged):** start-time `l_name` = DT_SONAME; TLS of dlopen'd objects (DL-16d); one
`UniqueTable` per call (DL-16b); start-time `$ORIGIN`; requester chain; fork/lock (DL-16c); dlerror record not
freed at thread exit (DL-15 hook); static `dlopen(NULL)` gives the static text with an empty name (DL-16S).

**Do not redo:** the probes, the driver, START_ARGS, the selector analysis above, the records.

**Next step:** coordinator assigns the ids; then DL-16b (dlsym) stacks on this branch.

**Plan rows to read:** plan/ph14/dl-16-dlopen.md §1-§4, §8; D-27.7, D-27.30, D-27.31, D-27.34; spec/behaviour/dynlink.md
"dlerror texts" and P-10, P-13.

Jobs on `cce283898` (shiki): `dl-dlopen-load-3-gate2` direct tier, rc 0, 11700 passed / 0 failed, conformance 130/0, peak 16G; `dl-dlopen-load-3-plants2` 7 CAUGHT / 0 SURVIVED / 0 WARN; `dl-dlopen-load-3-t4` clippy + static + the two ld test targets, rc 0. Superseded: `-lint0` (inherited head, rc 0), `-sel`, `-t1` (older heads), `-gate`/`-plants` (stopped to add tests), `-t2` (clippy items_after_test_module), `-t3` (trace: test name), `-recnew` (record skeletons).
