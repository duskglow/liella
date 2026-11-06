# DL-SYMBOLS (phase 14 DL-9): symbol lookup

**State:** branch `task/dl-symbols` from origin/next `349b94d0`; role mei:~/w/s2. CHECKPOINT hand-back (context
~150k): code, tests, probe record, D-27.11 and plants file written and pushed; the final head's jobs were started
detached, not yet read.

## Done
* `crates/ld/src/symbols.rs`: `sysv_hash`, `gnu_hash`; `Object` (lookup through the GNU table when present, else
  SysV, or a chosen `HashStyle`); `Want::{Oldest, Default, Version}`; `Object::reference_want` (versym + verneed);
  `Scope::{global, group(deepbind), next, excluding}`; `lookup` with the `UniqueRegistry` trait for
  `STB_GNU_UNIQUE`; `resolve` (weak undefined binds 0). Typed `LookupError`, no panic, no allocation.
* `crates/ld/src/elf.rs`: `Dynamic::symtab`, `Symtab::get` (count bound, `DT_SYMENT` = 24), `Sym`, STB/STT/STV/SHN.
* `crates/ld/tests/symbols.rs`: corpus built with the system linker (libv in GNU, SysV and both hash styles,
  unversioned stub, weak/strong, unique, two programs); 9 tests (probe bindings, GNU/SysV agreement, programs'
  references, weak, unique, scopes, malformed tables, byte-flip no-panic).
* Probe (black box, mei, scratch only): record in `spec/behaviour/dynlink.md` "Symbol binding"; rules in D-27.11.
* Plants: `handoff/DL-SYMBOLS-plants.txt` (19 plants, each with `msg:`).

## Jobs (mei)
* dl-symbols-t1 (first head 0c88f03d): 8/9 passed; `programs_bind_like_the_reference` failed because the test
  forced the GNU table on the SysV-only library (test bug, fixed: `Object::new` picks the table). SUPERSEDED.
* Final head: `dl-symbols-gate` (`cargo test -p liella-ld --test symbols`, `cargo xtask clippy`, `cargo xtask static`,
  `cargo xtask affected --base origin/next --run`) and `dl-symbols-plants` (`/mnt/zram/claude/DL-SYMBOLS/plants-1`),
  started detached; NOT READ at hand-back.

## Arch seam: ticked
No architecture literal added; lookup uses no `Arch` item; the test is `cfg(target_arch = "x86_64")` with an
`ARCH(x86_64)` comment (host-built corpus). Checklist items 1-8: nothing new per architecture.

## Decisions in force
D-27.7, D-27.8 (reader), D-27.11 (lookup rules, this story). R14-12 (no special version node).

## Open questions / rulings
* RULING NEEDED: a program that requires versions from an object without version tables. The reference loader warns
  twice and then stops with an internal consistency message (exit 127). Liella's lookup matches any definition
  in such an object. Recommendation: the DL story that adds the load-time version check keeps the warning and
  continues (the gABI makes versioning optional). This needs a differences row and a COMPAT option, or a recorded
  reason for not offering one.
* `Scope::next` for a caller that is only in a local scope (after it in that scope) is this story's reading of
  design §5; not probed.
* Not probed: a symbol whose lowest version is not VER_1 (the rule "lowest index" is consistent with every probed row).

## Next step
Read `~/logs/dl-symbols-gate.log` and `~/logs/dl-symbols-plants.log` on mei; fix anything red (clippy is the most
likely, since it was not seen on the final head); then hand back COMPLETE. Later: fuzz target `elf` gains lookups
(DL-ELF-READ-3 next step); L14-7 memo (per-object version-index table) in DL-10.
