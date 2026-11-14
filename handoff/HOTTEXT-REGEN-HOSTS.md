# HOTTEXT-REGEN-HOSTS(-2,-3): the hot-text list must not depend on which host or profile traced it

Branch `task/hottext-regen-hosts-3` (continues `task/hottext-regen-hosts-2`). HEAD: see handoff/reports/HOTTEXT-REGEN-HOSTS-3.json.
**State: SUPERSEDED by next. Nothing of this story's code is left on the branch.** After the merge of origin/next
(allowed by the -3 brief) the branch differs from next only in handoff/ (this note and the reports of -1, -2, -3).

## Why nothing was re-applied
The -3 brief said: merge next, keep next's lists and tooling, re-apply only this story's host-independence change.
Next already does all three parts of it, by another route (LAYOUT-TRACE-AT-BUILD, LAYOUT-DELETE-LISTS LT-5,
LAYOUT-TRACE-HOST-CANONICAL LT-4; decisions D-X.88, D-X.91, D-X.92):
- (1) Clean environment for the probes: tools/hottext/runner.sh `run_traced` passes only the variables the caller
  names (HOT_TEXT_ENV_*); trace_gdb.py also fixes cwd, ASLR, argv[0] and the host data files (canon_host.sh).
- (2) Host-class independence: this story's union over CPU classes with `<profile>@<class>` provenance (D-X.61) is
  replaced by D-X.91: every probe runs once on each of four virtual CPU classes (v3-erms, v3, base-erms, base) and the
  runs are merged, so a key used only on one class (fill_erms, the case this story found) is grouped on every host.
  D-X.91 measured byte-identical lists and products on a Zen 1, a Skylake-SP and a Haswell-EP host.
- (3) Per-profile lists: no list is committed any more (LT-5). `cargo xtask so` traces its own product and writes the
  lists to target/product/<profile>, so a fast trace cannot reorder the release layout (the cause of
  hrh-regen-keke-r1).

Merge resolution: next's side for every conflicted path (CONTRIBUTING.md, hot_text_products.rs, regen.sh, hottext.rs,
layout.rs, so.rs, startup_data.py; next's deletions of hot-text.absent, startup-text.keys, merge.py, prune.py kept).
Also removed: the split lists hot-text-fast.list / hot-text-release.list (LT-5 commits no list), the -1/-2 plants
files (their targets, merge.py and the split-list code, are gone), and records/decisions/D-X/D-X.61.toml (never landed,
describes a design that is not in force; the records only have `status = "active"`).

## Jobs (keke, role n2)
- hottext-regen-hosts-3-lint0: clippy + static on the inherited head 6a56f31dc (brief step 0).
- hottext-regen-hosts-3-lint, hottext-regen-hosts-3-gate: clippy + static and the direct-tier gate
  (`affected --run --base origin/next`) on the merged head. No plants: the branch changes no code.

## Not to redo
The -1/-2 work (merge.py union, per-profile split, release regen on both classes) is obsolete: do not port it.
Step 5 (`#[inline(never)]` on start-up keys) stays retracted (#4330, D-X.56).

## Next step
None for this story: close it once this branch lands (or close it without landing; the branch carries only notes).
Open item for the coordinator: the id D-X.61 stays unused.
