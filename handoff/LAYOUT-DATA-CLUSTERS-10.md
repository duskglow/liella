# LAYOUT-DATA-CLUSTERS-10: next merged, startup-data.keys regenerated on the merged tree

**State (2026-11-11):** branch `task/layout-data-clusters-10` = -9's `102f622e6` + merge of origin/next
`be4825645` (`c7c3a9a04`; the conflicted generated `crates/libc-so/startup-data.keys` took next's copy) +
`1b06b4efd` (keys regenerated, plants file) + this handoff. Role kanon:~/w/kn1. Scratch on kanon:
`~/work/LAYOUT-DATA-CLUSTERS-10/` (`regen.sh`, `final.sh`, `startup-data.keys.new`).

## Decisions in force
D-X.70 (the declared list), D-X.64 (clusters), D-X.50. No new record. -9's changes kept unchanged by the merge
(declared `.bss` objects in `startup-state.list`, LOCALE_BOUND 52 / TOOL_BOUND 56, the plants file).

## How the keys were regenerated (no hand-picked lines)
`tools/hottext/startup_data.sh .` (merged tooling, i.e. the branch's alias-keying `startup_data.py`) on the
merged tree: release products first, then fast; each run drops its profile from every old entry and adds what it
traced, so both profiles' entries are rewritten and an entry neither profile traces goes. Job `ldc10-regen2`
then built both profiles with the result: STARTUP-DATA-WINDOWS ok (fast 1 ro window / 12 rw pages, 53 of 53
objects; release 1 / 14, 73 of 73). Diff against next's copy: + `program_invocation_name`,
`program_invocation_short_name`, `environ`, `_environ` (aliases, the branch's keying),
`liella_machine_strcmp_rescans`; - `__bss_start` (size-less marker), and fast-only `_IO_2_1_stderr_`,
`_IO_list_all`, `stderr`, `classed::OUTCOME`, `dispatch::FMA`, `__rseq_offset`, `__rseq_size` (no longer
traced in the fast `page_faults true`).
Do not redo: `startup_data.sh --replace` for one profile drops the other profile's keys, and that profile's
`cargo xtask so` then fails STARTUP-DATA-WINDOWS ("no start-up object traced in profile release"; job
`ldc10-regen`, rc 1). Regenerate in merge mode, one run per profile.

## Jobs (kanon)
ldc10-lint0 (inherited head: xtask clippy + static, rc 0); ldc10-regen (superseded, see above); ldc10-regen2
(rc 0); final head 1b06b4efd: ldc10-lint (workspace clippy -D warnings, xtask clippy, static), ldc10-gate
(direct tier, --base origin/next), ldc10-hot (`cargo xtask so` + hot_text_products), ldc10-plants. Results:
see `handoff/reports/LAYOUT-DATA-CLUSTERS-10.json`.

## OPEN: plant ldc9-locale-set SURVIVED (ldc10-plants rc 1)
On the merged head (kanon, Broadwell-EP, fast products) dropping the setlocale set from `startup-state.list`
no longer fails `init_short_programs` / `init_coreutils_shaped` (setlocale + printf stays <= LOCALE_BOUND 52);
-9 caught it on mei (Skylake-SP, pre-merge tree, 51 -> 53). Not diagnosed (budget): the passing test prints no
count. Either next's code moved what shares the last `.bss` page (the set no longer alone there), or the
count is host-dependent. Next: measure setlocale + printf with and without the set on this head (mei and
kanon, fast; -9's `m2.sh` recipe), then either re-aim the plant/test at the faults the set now saves, or
tighten LOCALE_BOUND if the head now reads lower (ALWAYS FORWARD). Do not loosen any bound.

## Next step
LAYOUT-TEXT-PIN-14 (waits for this story; the absolute-address scan in startup_data.py is its work, not done
here). Open from -9: release-profile locale / tool counts (expect 51 / 55); fold the two `.data1` list tests
into one; extend `init_startup_data_accesses_are_in_the_data1_cluster` to trace `relay locale` and `relay tool`.
