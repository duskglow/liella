# LAYOUT-STARTUP-STATE-LOCALE: deterministic start-up state order (#4923), setlocale's objects (#4915), first-call cluster (#4942)

**State (2026-11-12, run -4):** branch `task/layout-startup-state-locale-4` (= -3's head df59a2829 + run -4's commits;
no merge). Role keke:~/w/n1. Scratch on keke: `~/work/LAYOUT-STARTUP-STATE-LOCALE-4/` (`branch.sh` = -3's for this
role; `pads.py`, `faultmap.py`; next's saved products `next-{fast,release}` copied from mei's -3 scratch; this head's
products `b1-{fast,release}`).

## Decisions in force
Rulings #4923 (order), #4942 + #4949 (first-call tiers in a page-aligned `.lbss` cluster after `.bss`; `.ldata` would
start a second writable PT_LOAD, D-X.68), #4943 (relocation sections move with their targets), D-X.70, D-X.64, jitter
rule #4347. Records (run -4, #4950): `records/pending/LAYOUT-STARTUP-STATE-LOCALE-4-1.toml` (order rule) and `-2.toml`
(first-call cluster), both `D-NEW`, series D-X: the coordinator assigns the ids.

## Done
- -1/-2: list-order pass (`order_sections`, relocation sections follow), the rename after the copies pass's `ld -r`
  join, the access check over all ten probe modes, the order test.
- -3: `cluster first-call` line in `startup-state.list`; `coldbss.rs` `state_plan` renames the Rust object's
  zero-filled first-call sections `.lbss`, after the every-process tier and ahead of the cold tables, the first one
  aligned to 4096; initialised first-call objects and `csys_7.o`'s stay in `.data1`. Order test covers both clusters.
- -4: **cause of lssl3-branch2 rc 3**: intl::RESOLVED is page aligned by its own type (`#[repr(C, align(4096))]`,
  GETTEXT-SLOTS-PAGE) and is first-call state, so the cluster always held two 4096-aligned `.lbss` sections and the
  "exactly one" check could never pass. Lowering RESOLVED's alignment would undo GETTEXT-SLOTS-PAGE (ALWAYS FORWARD),
  and joining the sections would lose the list order inside the cluster; the check now asks what the cluster needs:
  the first `.lbss` section in index order (the link order) carries the page alignment (`cluster_starts_page`, unit
  cases in `declared_startup_state_is_found_whatever_its_symbol_suffixes`). Commit da8343fd6.
- -4: the two decision records (commit 86ad95636); plants file: lssl3-align's message follows the new wording,
  new plant lssl4-any-aligned (the check accepting any aligned `.lbss` instead of the first).

## Measured (lssl4-branch1, keke, Zen 1 host, head da8343fd6; cells next-fast / b1-fast / next-release / b1-release)
21 pads (orig, pre 0x800-0xf000, cold, data, bss, lbss): `true` equal to next at every pad and profile (11/11/10/10
at orig); locale 1-2 lower (50/49/51/50 at orig), tool 2-3 lower (54/52/55/53), fork, mktime, localtime(_mktime),
float, strings, rebind equal everywhere. Sums over pads: fast locale 957 -> 934, tool 1033 -> 991; release 974 -> 951,
1050 -> 1008; true 212/212, 192/192. WORSE cells: 0. xtask unit tests and the four hot_text_products start-up state
tests pass in both profiles. The `pre` pads move `true` 11 -> 12 on next and the branch alike (text-side jitter,
#4347, not this story's state).

## Jobs (keke)
lssl4-lint0 (inherited head: clippy + static, rc 0), lssl4-branch1 (da8343fd6: products, tests, pads, rc 0),
lssl4-lint1 (86ad95636: records check + clippy + static, rc 0); lssl4-gate (86ad95636, `affected --run --base
origin/next`, escalated itself to FULL because xtask/src changed: 32 suites ok, 11643 passed, 0 failed, 1269
ignored; conformance 130/0; peak 13G, rc 0; includes the whole hot_text_products);
layout-startup-state-locale-4-plants2 (9 plants: 9 CAUGHT, 0 SURVIVED, 0 WARN, rc 0; per-plant list in the plants
file). Superseded: layout-startup-state-locale-4-plants (stopped by me after 4 plants: lssl-emission-order gave
WARN-NO-MSG because the build's new check now refuses that mutant first; the file was fixed and rerun as plants2).
Commits after 86ad95636 touch handoff/ only.

## Open / next
- Coordinator: assign the two pending D-X ids (records/pending/LAYOUT-STARTUP-STATE-LOCALE-4-{1,2}.toml).
- Nothing else is left in this story.
