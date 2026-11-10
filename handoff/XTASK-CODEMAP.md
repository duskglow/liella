# XTASK-CODEMAP: `cargo xtask codemap` regenerates docs/codemap from the tree

State: branch `task/xtask-codemap` (continues CODEMAP-ASM-EXPORTS; origin/next merged in); code head 05b8c5f86.
Report json: handoff/reports/XTASK-CODEMAP.json. Plants: handoff/XTASK-CODEMAP-plants.txt.

## What was done

- `xtask/src/codemap.rs` rewritten: `cargo xtask codemap [--check]` writes both `docs/codemap/symbols.tsv` and
  `docs/codemap/README.md`. `codemap-asm` remains as an alias. The rules are in the module doc. In short:
  - symbols.tsv rows (sorted bytewise): every `pub extern "C" fn` in `crates/boundary/src/` whose item is an
    `export! { [kind] "V";` (version V; `local` = hidden name, no row), an `internal_entry!` or a `no_mangle` attribute
    (version empty). An `export! { asm }` row names the `.S` global label and its first `call`/`jmp`/`jcc` target. A
    `.symver STUB, NAME@V` alias row points at the Rust function the stub jumps to (or else the stub label). Asm-only
    global labels without a `liella_` prefix (vfork, setjmp, getcontext, ...) get a row. So do `LIELLA_VFWD` forwarders
    in `csys/src/adapter/*.c` and the s10_math.tsv families (`(math-gen)`). An asm export without a global label, or a
    `.symver` name that nothing defines, is an error.
  - README: the header, the section table, the differential tests, the crates, system-file paths and shared helpers
    are generated. Everything from the first `## ` heading the generator does not write to the end of the file
    (the assembly checklist, Layout contracts) is kept verbatim. A dead repository path in that part is an error.
- `cargo xtask static` runs `codemap --check` (about 0.2 s on the build host): "... out of date: run cargo xtask codemap".
- Merge conflict with origin/next resolved: main.rs by hand (both sides kept); docs/codemap by REGENERATION only.
  All 40 `export! { asm }` rows now name their label and first branch target (pthread_once ->
  once.S:77 `liella_machine_once`). No row names the removed rcmd.rs file.
- Tests: 5 fixture tests in codemap.rs. They cover Rust and asm rows, a .symver row and an asm-only row, the
  hand-written section being kept, a missing label as an error, a removed file leaving no row (plus the check seeing
  it), a dead path in the hand-written part, and the first-call rules.

## Differences from the old map (INFORMATIONAL, by design)

- 4044 rows (was 3315). New rows: compat `.symver` aliases (410 lines in compat_aliases.S), asm-only exports, and
  `libm`/`runtime` export rows whose version was empty before (the version now comes from the macro).
- Dropped: `__isoc23_`, `__isoc99_` and `name`, which were artefacts of the old parser, and `default_obstack_failure`,
  which is not a C symbol (no `no_mangle`).
- First call: the first qualified call of the body, the same for every row (for example `mret::pure`, `ret::null`).
  Math-gen rows no longer skip `mret::` wrappers. The old generator did that for tsv rows only.
- The clause-test column now lists only `sNN_*` test files. The two hand-added math test paths for §10
  (special_cr.rs, mp_kernels.rs) are no longer listed. To list them again, add a rule, not a hand edit.

## Next step

- The dispatcher reads the gate (xtask-codemap-gate). If it is red, fix it and rerun only the failing suite.
- Optional: take the versions of asm-only rows (vfork, setjmp) from spec/abi/versions.tsv; they are empty now.
