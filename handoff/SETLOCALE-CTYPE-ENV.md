# SETLOCALE-CTYPE-ENV: handoff

State: branch `task/setlocale-ctype-env` (from origin/next 19c0ea167); code head 54089d888 (handoff commit on
top). Build role mei:~/w/s1 (Skylake-SP). Decision: records/pending/SETLOCALE-CTYPE-ENV-1.toml (D-NEW, series D-X,
as the brief's change section says; its Rules section's template names D-10; the coordinator assigns the id).
Percall spec in the tree: validation/percall/setlocale.c (the six rows of SETLOCALE-POSIX-ATTR plus LC_ALL "" and
two LANG-only rows).

## Cause (profiled before the design)

wl_trace call 11 of `setlocale(LC_CTYPE, "")` on next (release): 9,562 instructions, ~8,500 in the environment walk
inlined into setlocale (strnlen 1,827, machine_equal 1,017). All fourteen LC_*/LANG names start with `L`, so every
`L` entry was measured and compared once per name, and the walk always ran to the end. The load was already
skipped for a kept name (replace_changed, D-X.83); the locked path was ~1,000. So the brief's premise ("reload each
call") was only the smaller part.

## Change

- `global_locale::setlocale_env_kept`: lock-free no-change answer from the query cache (names resolved from the
  environment on every call, compared with the current names, "POSIX" as "C").
- Boundary: `setlocale(cat, "")` out of line (`setlocale_env`); one category reads LC_ALL, LC_<cat>, LANG lazily
  through getenv's scanner; LC_ALL reads LC_ALL, then walks for the other 13 only when it is unset or empty.
- `environ::getenv_many`: a candidate entry read once (longest name + `=`), names compared only at the entry's `=`.
- Hot text: the moved closure's key dropped with `cargo xtask hottext-repair --at 6cf3b90bc` (+ its unpinned line).

## Results (wl_percall release, mei, instructions/call; base 19c0ea167 / new 5ae7a4d5f (same code as head) / glibc)

query 76.0/70.0/44.0; C 147.3/124.3/156.1; all C 141.6/119.6/156.2; POSIX 160.3/137.3/2120.0; all POSIX
154.6/132.6/2643.1; LC_CTYPE "" 8254.8/587.1/2551.0; LC_ALL "" 8804.6/1093.0/11401.0; LANG only: LC_CTYPE ""
7067.2/919.0/6225.1, LC_ALL "" 8466.2/7180.0/21088.6. No row slower. Job setlocale-ctype-env-percall-4.

## Test and plants

crates/boundary/tests/setlocale_env_repeat.rs: LANG (C -> C.UTF-8 -> POSIX), LC_ALL set/unset/empty and LC_<cat>
over LANG changed between calls are seen (14 checks, all pass), counts printed. Plants
(handoff/SETLOCALE-CTYPE-ENV-plants.txt): env-not-reread (no-change compare skipped), walk-length-only.

## Open (exact next step)

The test program (fast profile, ~10-entry environment, LC_ALL set but EMPTY, LANG=C.UTF-8) counted LC_CTYPE ""
1,074 and LC_ALL "" 9,527 per call against query 85: LC_ALL "" there is near the base's locked-path cost, and the
LANG-only LC_ALL row is still 7,180. Not explained in this run (budget). Next: add a percall case with
`setenv("LC_ALL", "", 1)` and wl_trace it and setlocale_all_empty_lang (`--spec validation/percall/setlocale.c`) to
see whether setlocale_env_kept misses there (query cache slot, or the walk); then set a cost bound in the test
(a plant with `.filter(|_| false)` on setlocale_env_kept's result must be caught).

## Notes

- One heredoc used for a python edit early on (rule: no heredocs) - no effect on results.
