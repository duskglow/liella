# DL-DLSYM-2 report (continuation of DL-DLSYM, phase 14 DL-16b)

Branch `task/dl-dlsym-2`; role shiki:~/w/sh1. Summary and next step: handoff/DL-DLSYM.md.

## Commits (this run)
- `59b3d5117` merge of `origin/next` (ruling #5331; clean, no conflicts; DL-DLOPEN-LOAD-3 stays the base). The merge
  commit carries git's default message (no trailer); every other commit has one.
- `ebc13e949` Make dlvsym refuse unversioned objects and flip handles.c to passing.
- `c72ed99ff` Flip s27_dlopen_failures to passing and record the object capacity.
- the handoff/report commit on top.

## Why `dl-dlsym-t1` failed (rc 101)
The test linked its two start-time helpers with `-l:libh1.so` and a `DT_SONAME`, so the program's `DT_NEEDED` were
bare names; Liella's start-time loader opens `DT_NEEDED` by path only (`ld.rs`: "the library search is not available
in this build (name it by path)"). The helpers are now built without `DT_SONAME` and linked by path, as the existing
dlopen test and the conformance harness do. No loader change was needed for that.

## Defect found and fixed
Check 10, `dlvsym(RTLD_DEFAULT, "f", "NOVER_1")` with `f` defined by an object without version tables, answered the
definition. spec/behaviour/dynlink.md (probe-settled row): "`dlvsym` missing version: `<object>: undefined symbol:
<name>, version <version>` (also for an object without versions)". `Want::Version` (dlvsym, and the unique table's
lookup of a definition's own version, keyed by that definition's version) now matches only a definition of exactly
that version; a versym-less definition reads as base (index 1) and never matches. Relocations use `Want::Reference`
and keep D-27.11's permissive rule. `crates/ld/tests/symbols.rs::versions_bind_like_the_reference` now states both
halves. Pending record `records/pending/DL-DLSYM-2-2.toml` amends D-27.11; `-1.toml` records the DL-16b design.

## Tests and results
- `dl-dlsym-2-lint0` (inherited head a31883dd8): clippy + static clean.
- `dl-dlsym-2-t2`: `cargo test -p liella-ld --no-fail-fast`: all pass (dlsym 2/2, symbols, dlopen, ...).
- `dl-dlsym-2-t5` (head c72ed99ff): records check, `xtask so`, `s27_dynlink_clauses` (s27_1_c_3, s27_1_f_0 both
  passing, not pending), `liella-ld` dlsym + symbols, `cargo xtask clippy`, `cargo xtask static`: rc 0.
- Plants `dl-dlsym-2-plants` (head c72ed99ff): 12 CAUGHT, 0 SURVIVED, 0 WARN.
- Gate `dl-dlsym-2-gate` (affected --run --base origin/next; escalated itself to FULL because the branch changes
  xtask/src/compat.rs): RED, one test: `liella::hot_text_products init_data_clusters_hold_at_every_text_phase`,
  "tool: fault count moves with the text phase": tool row 53 minor faults at every pad except `cold 0x2000` = 54
  (`dl-dlsym-2-hot`, the test alone, same result). The same test on origin/next `1700e1798` (fresh tree in RAM,
  `dl-dlsym-2-next-hot`): passes (53 at every phase). Not claimed as LAYOUT-JITTER: the branch touches start-up
  code (`crates/rt/src/start.rs` stores `StartInfo::dlsym`, run 1; `crates/rt/src/tls.rs` and other start-time
  changes come from the unlanded base DL-DLOPEN-LOAD-3), so ALWAYS FORWARD applies. Not isolated: whether the base
  alone (DL-DLOPEN-LOAD-3 + next) already moves it. Stopped at the context checkpoint.
- Clauses: s27_1_c_3, s27_1_c_1_3 (program handle), s27_1_g_0 (unique, IFUNC, versions), s27_1_c_5 (texts) in
  `crates/ld/tests/dlsym.rs` (45 checks, both kernel-started and `ld.so PROGRAM`); s27_1_f_0 static entries.

## Plants (handoff/DL-DLSYM-2-plants.txt, 12)
RTLD_NEXT from a dlopen'ed and a start-time caller keeping the caller; RTLD_DEFAULT (NULL handle) from the program
skipping the program, from a dlopen'ed caller without the global scope and without its local scope; misaligned
handle accepted; success keeping the dlerror text; TLS answer off its block; IFUNC answered unresolved; unique table
bypassed; DEEPBIND global-first; dlvsym matching an unversioned object. Not planted: `pin_unique` (observable only
through `dlclose`, DL-17).

## Probe P-14 (#5321), reference glibc 2.39, Zen 1 host
N distinct copies of a tiny object, each `dlopen(RTLD_NOW|RTLD_LOCAL)` by path: N = 511, 512, 513, 1000, 2000 all
open. Liella holds 512 objects (start-time included) and refuses the next with `<name>: cannot load more objects`.
Recorded: dynlink.md row P-14, differences row (no compatibility option yet: the capacity is a table size).

## RULING NEEDED
- The start-up layout red above: measure DL-DLOPEN-LOAD-3 merged with next (same test) to place it; if the base
  shows it, it is the base's; if only this branch does, recommend treating it as LAYOUT-JITTER (#4347) since
  dlsym/dlvsym add no code on the start-up path beyond one stored address, else a layout fix in a continuation.
- #5321 capacity: recommend a growable object table as its own story (restores the reference's behaviour, so no
  switch); until then the differences row stands.

## INFORMATIONAL
- No instruction-count row covers `dlsym`; no start-up code changed, so no percall/count job was run.
- `s27_dlopen_failures` became passing through the merge (DL-DLOPEN-LOAD-3's loader + CONF-DYN-HELPERS' program);
  its pending mark was removed here, since run.sh fails a pending program whose Liella run passes.
