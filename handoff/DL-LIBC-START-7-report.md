# DL-LIBC-START-7 (continues DL-LIBC-START-6)

State: branch `task/dl-libc-start-7`, code head fd7612d31 (handoff commits on top).

## What was wrong
dl-libc-start-6-gate4 (mei) was red because step 5b of `cargo xtask native` compared native_dynamic's output
with the static example's 14-step EXPECTED; native_dynamic prints 13 steps. The missing step is `kernel types`,
which checks the re-export of `liella-kernel`'s public types. D-X.10 makes that re-export with feature `static`
only (the dynamic facade compiles in the stateless clean API and reaches the rest through `libc.so.6`), so the
step cannot exist in the dynamic facade without changing a decided public interface. Not a regression.

## Done
- xtask/src/native_examples.rs: `STATIC_ONLY = ["kernel types"]` and `expected(linkage)`: static = EXPECTED
  unchanged; dynamic = EXPECTED with its own first line, the static-only steps dropped, the count line recomputed
  (13 of 13). The summary line names both step counts. The static list is untouched.
- crates/native/examples/native_dynamic.rs: module docs corrected (it runs under Liella's ld.so; no kernel types).
- records/pending/DL-LIBC-START-7-1.toml: D-NEW, series D-10, amends D-10.5 (the dynamic `__fpu_control` hook
  now runs and passes under Liella's ld.so, via D-27.27). Needs an id from the coordinator.
- Plants: handoff/DL-LIBC-START-7-plants.txt (3 plants on the new code, `cargo xtask so && cargo xtask native`).

## Jobs (keke, role n2)
- dl-libc-start-7-gate: full-workspace clippy -D warnings, `cargo xtask static`, `cargo xtask affected --run
  --base origin/next`. Result: see the hand-back.
- dl-libc-start-7-plants: `cargo xtask plants handoff/DL-LIBC-START-7-plants.txt`. Result: see the hand-back.
- Counts: not run. This change touches no product code (xtask, an example's doc comment, a record), so libc.so.6,
  libc.a and ld.so are byte-for-byte -6's; dl-libc-start-6-counts-head4 (mei) measures them.

## Decisions in force
D-X.10 (facade features), D-10.5 + pending amend, D-27.24 (step 5b), D-27.27.

## Open
- RULING NEEDED (unchanged): #4807, Liella's own `libc_nonshared.a` (owner keeps the host archive for now).
- Should the dynamic facade re-export `liella-kernel`'s types too? That is a D-X.10 interface change for the
  owner; not needed for this story.

## Next step
Assign the D-10 id to records/pending/DL-LIBC-START-7-1.toml; land.

## Refusals
None.
