# DL-SYMBOLS (phase 14 DL-9): symbol lookup

**State:** COMPLETE. Branch `task/dl-symbols-2` (continues `task/dl-symbols`, from origin/next `349b94d0`); code head
`3a488d14`. Role shiki:~/w/sh1 (DL-SYMBOLS-2); the first run used mei:~/w/s2.

## Done
* `crates/ld/src/symbols.rs`: `sysv_hash`, `gnu_hash`; `Object` (lookup through the GNU table when present, else
  SysV, or a chosen `HashStyle`); `Want::{Oldest, Default, Version}`; `Object::reference_want` (versym + verneed);
  `Scope::{global, group(deepbind), next, excluding}`; `lookup` with the `UniqueRegistry` trait for
  `STB_GNU_UNIQUE`; `resolve` (weak undefined binds 0). Typed `LookupError`, no panic, no allocation.
* `crates/ld/src/elf.rs`: `Dynamic::symtab`, `Symtab::get` (count bound, `DT_SYMENT` = 24), `Sym`, STB/STT/STV/SHN.
* `crates/ld/tests/symbols.rs`: corpus built with the system linker; 9 tests. DL-SYMBOLS-2 added a weak undefined
  `x_undef` to the corpus library and checks that neither of its tables finds a definition for it (the GNU table never
  indexes undefined entries, so the old check could not catch the undef-def plant).
* Probe record: `spec/behaviour/dynlink.md` "Symbol binding"; rules in D-27.11.
* Ruling #3655 (coordinator, decided): a program that needs a version from an object without version tables is
  REFUSED like the reference loader (exit 127, a message naming the object and the version; no warn-and-continue, no
  COMPAT option). The lookup stays permissive. Recorded in D-27.11, in the DL-10 plan item (load-time version check
  before relocation, with its proof), in the probe record and as a comment at `Want::Version`.

## Jobs
* Final head 3a488d14, shiki: `dl-symbols-2-gate-2` rc 0 (full `cargo clippy --workspace --all-targets
  --all-features -D warnings`, `cargo xtask clippy`, `cargo xtask static`, symbols test 9/9, `cargo xtask affected
  --base origin/next --run` 67 passed, peak 966M); `dl-symbols-2-plants-2` rc 0 (19 plants: 19 CAUGHT, no WARN;
  `/mnt/zram/claude/DL-SYMBOLS-2/plants-2`).
* SUPERSEDED: mei `dl-symbols-plants` rc 3 (the tool refused the plants file: the gnu-hashcmp `lit:` pattern contained
  ` | `, the field separator; now a `perl:` pattern with `\|`); mei `dl-symbols-gate` (head bf829fa0); shiki
  `dl-symbols-2-gate` and `dl-symbols-2-plants` (head 2179d07c, green, but undef-def was WARN-NO-MSG: fixed by the
  test above); mei `dl-symbols-t1` (first head, test bug).

## Arch seam: ticked
No architecture literal added; lookup uses no `Arch` item; the test is `cfg(target_arch = "x86_64")` with an
`ARCH(x86_64)` comment (host-built corpus).

## Decisions in force
D-27.7, D-27.8 (reader), D-27.11 (lookup rules + ruling #3655), R14-12 (no special version node).

## Open questions
* `Scope::next` for a caller that is only in a local scope (after it in that scope) is this story's reading of
  design §5; not probed.
* Not probed: a symbol whose lowest version is not VER_1 (the rule "lowest index" is consistent with every probed row).

## Do not redo
Hash functions, version rules and scopes are probe-settled (dynlink.md); the plants file is complete for this code.

## Next step
DL-10: the load-time version check (now in its plan item) with the unversioned-stub program as its proof. Later: fuzz
target `elf` gains lookups (DL-ELF-READ-3 next step); L14-7 memo (per-object version-index table).
