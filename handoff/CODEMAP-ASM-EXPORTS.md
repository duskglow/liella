# CODEMAP-ASM-EXPORTS: asm-defined exports in the code map name their .S label

State: branch `task/codemap-asm-exports` (from origin/next 2281c1f6); HEAD in handoff/reports/CODEMAP-ASM-EXPORTS.json.

## What was done

- The generator named by `docs/codemap/README.md` (`<scratchpad>/tools/codemap.py`) is not in this repository, nor in
  liella-ops, nor anywhere on this machine (the scratchpad tools were lost 2026-10-19). So the fix is in xtask, where
  a check can run: `xtask/src/codemap.rs`, `cargo xtask codemap-asm [--check]`.
  - It reads every `export! { asm "V"; ... fn NAME` site under `crates/` and every global label (`.globl NAME` plus a
    `NAME:` line) in `csys/src/machine/*.S`. It sets the "defined at" column of each matching `symbols.tsv` row to the
    `.S` file and the label's line. An asm export that no `.S` file defines as a label is an error and is never guessed.
  - It reports each "defined at" path in `symbols.tsv`, and each repository path in `README.md`, that does not exist.
  - `--check` changes nothing. It fails on a stale asm row or a dead path, and it is part of `cargo xtask static`
    (check `codemap`).
- Regenerated `docs/codemap/symbols.tsv` with it: 38 rows moved (not 39; there are 38 asm export sites, each with one
  row). pthread_once / __pthread_once / call_once now name `csys/src/machine/once.S:77/85/93`, the lines of their
  labels. The escalation's hand-fixed rows are not on origin/next, so they could not be compared directly.
- `docs/codemap/README.md`: the `/etc/hosts.equiv` row pointed at the removed `crates/legacy/src/rcmd.rs:541`. I
  deleted that one row by hand, because the generator is not reachable and the new check would otherwise fail
  `static`. Nothing else in that file changed.
- Tests: 4 fixture tests in `codemap.rs` (asm export moves and Rust export stays; asm export with no label is an error;
  dead paths in tsv and README are errors; a label needs both `.globl` and a definition). Plants:
  `handoff/CODEMAP-ASM-EXPORTS-plants.txt` (7).

## Open / next

- RULING NEEDED: whoever runs `codemap.py` must run `cargo xtask codemap-asm` right after it (or port codemap.py into
  xtask). Otherwise the next regeneration brings the stub rows back, and `static` reports them. My recommendation is
  to port the generator into xtask (`cargo xtask codemap`), so the code map has a generator in the repository.
- INFORMATIONAL: line numbers in `symbols.tsv` have drifted throughout. For example, pthread_once's stub was listed at
  s22_threads.rs:986 but is now at :1035. The "first implementation call" column of asm rows still names the Rust
  stub's callee (e.g. `__pthread_once` -> `sync::call_once`). Only a full regeneration fixes both.
- Not to redo: the search for codemap.py (liella-ops, ~/w/.scratch, a filesystem-wide `find`).
