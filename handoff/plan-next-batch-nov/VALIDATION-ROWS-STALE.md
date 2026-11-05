# VALIDATION-ROWS-STALE: bring the stale validation triage rows to the tree's state

Id: VALIDATION-ROWS-STALE
Kind: docs
Priority: 45
Size: S
Host: -
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 25 tool calls / 60k

## The change

Gap: phase 16's table says the static-validation triage is "partly stale". Rows checked 2026-11-04: T23 says the libm
names "arrive with [Track M's] merge" (Track M is merged); T34's row still describes the `fmod` export although
PRE14-LIBC-FMOD reports it fixed on next (LIBC-A-PLACEMENT) and asks for the row's refresh; T24 still says "Pending
its signature" (see RULING NEEDED in summary.md: that one stays as is).
Scope: for T23 and T34 (and any other row whose status names a merge or story that has since landed), check the current
tree (`cargo xtask so`, then `nm`/`readelf` on `target/liella/libc.a` and `libm.a` for the names the row lists) and
rewrite the status cell to what the tree shows, citing the check. A row the tree does not settle stays as it is with
"rechecked <date>: unchanged". Files: `validation/README.md` only.
Proof: the commands and their summarised output in the hand-back; `cargo xtask static`.
JOBS: validation-rows-stale-so, VALIDATION-ROWS-STALE-static
