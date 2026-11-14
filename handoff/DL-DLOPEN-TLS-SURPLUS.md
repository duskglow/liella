# DL-DLOPEN-TLS-SURPLUS (DL-16d): dlopen of objects with PT_TLS into the static surplus

**State (2026-11-13, run 3 = DL-DLOPEN-TLS-SURPLUS-3):** branch `task/dl-dlopen-tls-surplus-3`, on origin/next
68e72ab32 (not merged: next has moved to 7b1d895b0; no merge allowed by the brief). Code head 02ea26118; this
handoff and the report json follow it. Role shiki:~/w/sh1 (Haswell). Run 2's gate (11737 passed, 0 failed) and its
plants (7/7 CAUGHT) on 427931bf2 were green on keke; run 3 removed the +2.1 thread_create_join cost.

## Decisions in force
- Pending record `records/pending/DL-DLOPEN-TLS-SURPLUS-2-1.toml` (series D-27, amends D-27.20, D-27.36): the whole
  design (one lock = libc's thread-list lock; `__liella_thread_walk` LIELLA_PRIVATE; table and dtv room for 512
  modules; eager dtv fill + lazy repair in `tls_slow`; place before relocation, rollback in `unwind`). Run 3
  updated its create-path sentence (below).
- Create path (run 3): the lock section of `create_inner` calls the loader's `tls_init` when `LOADER_INIT != 0`
  (set exactly when the image address is 0: `set_loader_tls` and a static program's `set_tls_image` are
  exclusive, both at start-up). The static image copy is back before the descriptor stores, outside the lock.
  `link_locked` inline; `list_insert` keeps its name, non-generic.
- `conv::store_words` / `store_words_user` take `[usize; N]` (const generic): the two start words are
  straight-line stores instead of a stack array + min + 2-iteration loop (about 20 instructions per create).
- DL-16c (DL-DLOPEN-CONCURRENT) overlap: at `load_set`'s publication point this branch adds only the
  `publish_tls` call (plus `place_tls` before relocation and one `rollback` line in `unwind`).

## Measured (shiki, Haswell; wl_percall static release products, instructions:u per call, sd 0)
| case | glibc | base 68e72ab32 | run-2 head bb5d32703 | hook test only 4c5036881 | + start words 7ea42ff12 / 02ea26118 |
|---|---|---|---|---|---|
| thread_create_join | 2274.0 | 35940.2 | 35942.2 (+2.0) | 35943.2 (+3.0) | 35922.2 (**-18.0**) |
Profile (wl_trace --list, one pthread_create, 18085 instructions): 17471 in one loop of `create_inner`, the
inlined `KEYS.span()` in `take_mapping` (reverse scan of all 1024 key sequence words on every mapping reuse);
the hook test is 3 instructions (load, test, branch), the old start-word copy ~22.

## Open questions / informational
- `KEYS.span()` (crates/rt/src/thread.rs) costs ~17k instructions per thread creation (about half the row,
  and the row is 15.8x glibc): a high-water mark kept at key creation would make it O(1). Proposed as its own
  perf story (not done here: outside this story).
- `dlsym` of `STT_TLS` in `dlopen`ed objects: DL-16b is not on 68e72ab32. Answer from the object's `Obj.tls`
  (`Placed { module, tp_offset }`, set by `place_tls`): TP + offset + st_value.
- `tls_slow` (lazy repair) and the create/dlopen race have no deterministic plant; 32 racer threads check it.
- Dynamic thread_create is not counted by wl_percall (static only).

## Not to redo
The design; the reading of DL-15; the Surplus maths. Shapes measured worse: image kept live into the lock
section (+2.0), hook test alone with the copy outside (+3.0): any late test costs a compare and a branch,
so the static path needs a saving elsewhere on it to stay at or below base.

## Jobs (run 3, shiki; code 02ea26118, later commits handoff/ and the record only)
- `dl-dlopen-tls-surplus-3-gate`: escalated to FULL by the tool (the branch changes xtask/src/so.rs, as in run 2):
  32 suites ok, 11737 passed, 0 failed, 1269 ignored; conformance 133 passed, 0 failed; peak 16G, rc 0.
- `dl-dlopen-tls-surplus-3-plants2`: 11 plants, 11 CAUGHT, 0 WARN (the 7 of run 2 + create-no-loader-tls,
  create-no-image-copy, start-words-one, clone-words-swapped). The first run (`-plants`) caught all 11 but the
  four new msg regexes guessed the harness wording (WARN-NO-MSG); fixed from the real failure lines.
- `dl-dlopen-tls-surplus-3-percall3`: thread_create_join 35922.1 (base 35940.2, glibc 2274.0).
- `dl-dlopen-tls-surplus-3-startup` (cbench_startup --compare --counters; Haswell, no limit row for this CPU):
  startup_instr_c 2147, setloc 67444, printf 71247, wctype 68468 (glibc 119766 / 145025 / 146800 / 145237).
  Static start-up runs no new code on this branch (an export and a constant); no base count on this host.
- `dl-dlopen-tls-surplus-3-lint2`: clippy + static + records check on the final head.

## Next step (exact)
Land it (after DL-16c and DL-16b, wire dlsym of STT_TLS as above). Separately: the `KEYS.span()` story.

## Plan rows to read
plan/ph14/dl-16-dlopen.md §5, §8 (DL-16d); D-27.20, D-27.36; the pending record above.
