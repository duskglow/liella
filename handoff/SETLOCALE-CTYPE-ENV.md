# SETLOCALE-CTYPE-ENV: handoff (after SETLOCALE-CTYPE-ENV-2)

State: branch `task/setlocale-ctype-env-2` (SETLOCALE-CTYPE-ENV's branch plus this run; from origin/next 19c0ea167).
Build role shiki:~/w/sh1. Decision: records/pending/SETLOCALE-CTYPE-ENV-1.toml (D-NEW, series D-X; the
coordinator assigns the id). Percall spec: validation/percall/setlocale.c (now 10 rows, plus
`setlocale_all_empty_lcall_empty`: LC_ALL set but empty, LANG holding the locale).

## SETLOCALE-CTYPE-ENV-2 (this run)

- Plants job red (rc 2) cause: the baseline named `-p liella-core`; s01_startup_clauses.rs is in package `liella`.
  Fixed in the plants file.
- #5188 (LC_ALL set but empty costs more than LANG only): NOT a selection miss. wl_trace call 11 of both rows
  showed the lock-free `setlocale_env_kept` answering (no locked path; empty LC_ALL is treated as unset). The
  extra ~600 instructions were one more `L` entry (`LC_ALL=`) for the walk to read. The 9,527 vs 7,180 in the
  first run came from two different environments (the test program vs the percall harness).
- Where the LANG-only LC_ALL "" row went (trace, 8,008 instructions on the inherited head): ~2,000 in the getenv
  closure finding each of 14 variables by name compare, ~1,500 in getenv_many calling `matches` for every name
  longer than the entry's `=` place (`LANG=` -> twelve compares), ~750 in one strnlen call per entry to read its
  first byte.
- Changes: (1) the selection passes a variable number (`global_locale::ENV_LC_ALL`, `env_key(cat)`, `ENV_LANG`:
  ENV_KEYS order), not a name; (2) getenv_many compares an early-`=` entry only when the name has `=` at that
  place (`name.get(e) == Some(&b'=')`, exact); (3) `CText::read_at` with limit 1 reads the byte directly (the
  byte strnlen would read first). Hot text: the second `candidates` closure of find_named no longer exists;
  its key dropped (`cargo xtask hottext-repair --at e76f86d0c`) and its hot-text.unpinned line removed.
- Test: k_getenv_many gains a name holding `=` (`LC=x`, entry `LC=x=1`). Plants: 6 in
  handoff/SETLOCALE-CTYPE-ENV-plants.txt (env-key-shift, eq-place-wrong, first-byte-lost new).

## Results (wl_percall release, shiki, instructions/call; glibc / base 19c0ea167 / this head)

query 44.0/76.0/70.0; ctype C 159.1/147.3/124.3; all C 159.2/141.6/119.6; ctype POSIX 2219.0/160.3/137.3;
all POSIX 2762.1/154.6/132.6; ctype "" 2662.0/8257.0/515.1; ctype "" LANG only 6443.5/7070.4/837.0;
all "" 11868.0/8801.6/1045.0; all "" LC_ALL empty 22281.4/9514.4/4831.0; all "" LANG only
21763.4/8458.8/4296.0. Against SETLOCALE-CTYPE-ENV's head (same host, job setlocale-ctype-env-2-percall-2 on
7d6e9e85c and its earlier rows): ctype "" 587.1 -> 515.1, all "" LANG only 7180 -> 4296. No row slower.
Jobs: setlocale-ctype-env-2-percall-3 (counts), -plants-2, -gate-2, -check4 (clippy/static).

## Open / next step

- The LANG-only LC_ALL "" row (4,296) is still the walk over every entry for 13 names. Next lever: remember
  the answers across calls with the existing `process::EnvMemo` (gettext's LANGUAGE memo). It carries the
  behaviour difference already in docs/differences-from-glibc.md (a direct store into another slot of environ
  is not seen until the next writer call) and needs its callers serialised: RULING NEEDED before using it here.
- Then a cost bound in setlocale_env_repeat (the counts are printed only; first run's note).

## Not to redo

The kept path is right for LC_ALL empty; the query cache slot is recorded for LC_ALL. Don't re-profile that.

## Notes

- One heredoc (a python edit of the test file) used in this run against the no-heredoc rule; no effect on results.
