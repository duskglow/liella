# SETLOCALE-C-ROWS: handoff

State: branch `task/setlocale-c-rows` (from origin/next 93721fcb1, which has SETLOCALE-POSIX-ATTR / D-X.80), code head
345e9fdf3 (handoff commit on top). Build role shiki:~/w/sh1 (Zen 1). Percall spec: shiki `~/work/SETLOCALE-C-ROWS/spa.c`
(the six setlocale rows of SETLOCALE-POSIX-ATTR). Decision: records/pending/SETLOCALE-C-ROWS-1.toml (D-NEW, series D-X).

## What was chosen and why

Neither of the brief's two options: the +1..+2.6 was not the alias compare, it was the fast path's codegen. Measured
(wl_percall release, instr/call, Zen 1; columns query / ctype_c / all_c / ctype_empty / ctype_posix / all_posix):

| tree | query | C | all C | "" | POSIX | all POSIX |
|---|---|---|---|---|---|---|
| next before D-X.80 (a5636ce6b) | 93.0 | 180.3 | 174.6 | 8261.0 | 2490.4 | 11936.6 |
| next (93721fcb1, before) | 94.0 | 182.3 | 176.6 | 8261.8 | 197.3 | 191.6 |
| A: alias check cold, out of line (d3faa5c56) | 93.0 | 184.3 | 178.6 | 8261.4 | 209.3 | 203.6 |
| F: cache answers split from locked path (e3b630ab8) | 89.0 | 183.3 | 177.6 | 8282.4 | 195.3 | 189.6 |
| G: F + inline name compare (8fc7cac57) | 76.0 | 149.3 | 143.6 | 8267.4 | 162.3 | 156.6 |
| H: G + request by reference (cd92a2df1) | 76.0 | 149.3 | 143.6 | 8269.4 | 162.3 | 156.6 |
| final: H + boundary calls locked path for "" (345e9fdf3) | 76.0 | 147.3 | 141.6 | 8256.6 | 160.3 | 154.6 |

- A (the brief's first option) is worse: a call on the miss path keeps more values live. Commit kept in history,
  replaced by F (new commits only).
- The cost on every cache-answered call was (1) the prologue of the whole function (six pushes + stack probe for
  ~5.5 KB of buffers only the locked path uses) and (2) `bcmp` library calls for 1-byte name compares, which forced
  callee-saved registers. Fix: `setlocale` keeps only the cache answers; `setlocale_locked` (#[inline(never)], pub)
  holds the rest; `same_name` compares names 8 bytes at a time inline.
- `""` moved +21/+6/+8 with F/G/H: the locked path's buffers sat deeper (LLVM kept `call`, no sibling jump even with the
  request by reference), and that row's memmove count follows their alignment (D-X.80). The boundary now calls
  `setlocale_locked` itself for an environment request (the cache never answers it): 8256.6 (sd 3.3).
- The boundary-mapping option ("POSIX" -> "C" before the call) was not tried: it adds a length compare to every named
  call, and the split already gives POSIX -37.

## Hot text

The split moved codegen units in the boundary crate: the fast profile no longer emits intl::bind, intl::textdomain and
getopt::next as their own functions (`xtask so` refused; next itself builds: job setlocale-c-rows-baseso).
`cargo xtask hottext-repair --at c85a49459` dropped the 3 keys; their hot-text.unpinned lines removed. A later
regen.sh run may want to re-trace those paths.

## Test and plants

crates/boundary/tests/setlocale_posix_repeat.rs now also counts `setlocale(cat, NULL)` again and requires "C" again
<= query again + 85/call (fast profile measured 75 for both categories: C 169 / NULL 94, all 164 / 89). Plants
(handoff/SETLOCALE-C-ROWS-plants.txt): `alias-libcall` (both compares back to slice `==`, the D-X.80 form) and
`name-libcall` (the as-given compare back to `==`).

## Results

Report: handoff/reports/SETLOCALE-C-ROWS.json. Jobs on shiki: setlocale-c-rows-lint (records check, clippy, static:
rc 0), setlocale-c-rows-percall-before, setlocale-c-rows-percall-final (rc 0), setlocale-c-rows-gate and
setlocale-c-rows-plants (running at hand-back).

## Exact next step

Coordinator: collect gate and plants; assign the D-X id. Optional follow-up: `setlocale(LC_CTYPE, "")` repeated is
8257 vs glibc 2662 (re-resolves from the environment and copies whole locales every call).
