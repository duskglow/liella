# SETLOCALE-CTYPE-ENV: handoff (after SETLOCALE-CTYPE-ENV-4)

State: branch `task/setlocale-ctype-env-4` (runs 1-3 plus a merge of origin/next 8f3e2993e, merge commit
5a7f7b999). Build role keke:~/w/n1. Decision: records/decisions/D-X/D-X.93.toml. Percall spec:
validation/percall/setlocale.c (10 rows plus `setlocale_all_empty_lcall_empty`).
Ruling #5298 (coordinator): no EnvMemo for setlocale(cat, ""); the environment is read on every call.

## SETLOCALE-CTYPE-ENV-4 (this run): merge with trace-at-build next; one start-up bound red in fast

- Step 0 (clippy + static on the inherited head a224aa144): green (job setlocale-ctype-env-4-lint0).
- Merged origin/next 8f3e2993e. next had deleted crates/libc-so/hot-text.list and hot-text.unpinned (the build
  now groups from a trace of itself); taking next's side means deleting both. Every crates/libc-so file now
  equals next's. No keys regenerated or pinned, no bound changed.
- Start-up layout tests (hot_text_products, heap_meta_products, cold_bss_products), keke (Zen 1), traced default,
  minor faults, lowest of 5 (every run identical):

  | program            | bound | branch fast | branch release | next 8f3e2993e fast |
  |--------------------|-------|-------------|----------------|---------------------|
  | startup only       | 11    | 11          | 10             | 11                  |
  | setlocale + printf | 47    | 47          | 47             | 46                  |
  | coreutils-shaped   | 50    | **51 RED**  | 49             | 50                  |
  | fork child         | 5     | 5           | 5              | 5                   |

  Jobs: setlocale-ctype-env-4-layout (fast, rc 101: only init_coreutils_shaped), setlocale-ctype-env-4-layout-rel
  (release, rc 0, 26 passed), setlocale-ctype-env-4-nextfaults (next, fast, rc 0). heap_meta: first malloc 5/6
  (bound 12), 14 classes 9 (bound 9).
- Gate setlocale-ctype-env-4-gate (`affected --run --base origin/next`, merge head): rc 1, the same single test
  (fast, plain and exports-interposed); every other step ok (conformance 125/125, liella-boundary,
  liella-legacy, liella-math, xtask unit tests, clippy).
- Reading: in fast products this branch adds one start-up page to both setlocale programs (next sits at the
  coreutils bound already, 50/50, so the +1 goes over). The branch does change code those programs run
  (setlocale_env, its getenv walk, setlocale_locked), so this is not unrelated jitter; under ALWAYS FORWARD the
  code has to fit. Not investigated further in this run (the brief: report the table, do not change bounds).

## SETLOCALE-CTYPE-ENV-3 (this run): start-up faults

- Run 2's gate was red on hot_text_products (setlocale+printf 55 faults > 51, coreutils-shaped 58 > 56): the new
  setlocale_env path was not in the hot group. `tools/hottext/regen.sh . --both --allow-drop` (job
  setlocale-ctype-env-3-regen2) added setlocale_env, its getenv closure, setlocale_env_kept, setlocale_locked
  and CText::read_at to hot-text.list.
- Dropped keys (why --allow-drop): iconv `Conv::to_words` (fast) is now inlined into every caller: libc.a keeps
  its section but has no call relocation to it, only to its jump table (objdump -dr). The
  `global_locale::setlocale` monomorph (release) is inlined after run 1's split. Their hot-text.unpinned lines
  are removed. The first regen without --allow-drop refused on to_words (job -regen, rc 1).
- hot-text.unpinned, as `cargo xtask static` (check_pinned) required: find_named's candidates closure is hit
  again in fast, so its line is back as on next. The setlocale_env closure (taken as `&dyn Fn`, so it can't be
  pinned), setlocale_env_kept (inline in release, where a pin would add a call) and CText::read_at (also on
  getenv's scanner, environ.rs:102 and process.rs:216, where a pin may add a call) get new lines.
  textdomain's line moves from tier extra to tool, following the list. I tried `#[inline(never)]` on read_at
  and reverted it for that reason. No code changed in this run.
- `cargo test -p liella --test hot_text_products init_` (job -faults, fast profile): 22 passed, including
  init_short_programs and init_coreutils_shaped. The counts weren't printed (captured output), only that
  they are within bounds.

## Results from run 2 (wl_percall release, shiki, instructions/call; glibc / base 19c0ea167 / head)

ctype "" 2662.0/8257.0/515.1; ctype "" LANG only 6443.5/7070.4/837.0; all "" 11868.0/8801.6/1045.0;
all "" LC_ALL empty 22281.4/9514.4/4831.0; all "" LANG only 21763.4/8458.8/4296.0; C/POSIX/query rows
lower too. No row slower. This run changed only the layout lists, so those counts still stand.

## Open / next step

- FIRST: bring the fast coreutils-shaped count back to <= 50 without touching the bound. Look in the fast
  products' trace (target/product/fast/trace.log) and a link map of page_faults.c (xtask/src/layout.rs
  near line 676 links it that way) for the page the new setlocale_env path adds in fast; candidates: CText::read_at or the
  setlocale_env closure out of line in fast (release inlines them), or new code past the first fault-around
  window. Measure next and the branch with the same job (one-line diff of the fault-address lists).

- A cost bound in setlocale_env_repeat (the counts are printed only).
- The LANG-only LC_ALL "" row (4,296) is the walk over every entry for 13 names. Ruling #5298 rules out a
  memo; a cheaper walk (for example skipping entries not starting with `L` before any compare) is the
  remaining lever.

## Not to redo

The kept path is right for LC_ALL empty; the query cache slot is recorded for LC_ALL. #5188 is settled (one more
environment entry, not a selection miss). Don't re-profile either.

## Notes

- Run 3: one empty heredoc slipped into a shell command (a no-op `python3 - <<X`), against the no-heredoc rule.
  I also used one `git show origin/next:crates/libc-so/hot-text.unpinned | grep` to find next's find_named line
  (it printed nothing). The line was then taken from this branch's own diff.
- Full report: handoff/reports/SETLOCALE-CTYPE-ENV-3.json; plants: handoff/SETLOCALE-CTYPE-ENV-plants.txt.
