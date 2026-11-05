# PERF-GETTEXT-LANGUAGE-STEADY(-4) (branch task/perf-gettext-language-steady-4, role keke:~/w/r2)

State: -3's work, ready to land once its jobs are green; target (found ~454) not reached. The branch is origin/next
f2157504 (which has XTASK-PLANTS-TRACKED-COPY) with -3's branch merged in (e579908c), then 48310c65 (the next change, below)
and its revert 14f130fd. **The tree at 14f130fd is the same as at e579908c** (`git diff e579908c 14f130fd` is empty), and
that is the tree the -4 jobs test (the role's .synced head is e579908c).
Library changes on the branch (all from -3, crates/legacy/src/intl.rs): `Resolution::matches` and `Resolution::hit`
#[inline(always)]. No new unsafe, no ABI or behaviour change. Decisions in force: D-7.5, D-7.12; saturation 14 row 6 runs 1-6.

## -4 jobs (keke; all queued at 16:46Z, none had started by hand-back: the queue was busy for over an hour)
- perf-gettext-language-steady-4-gate: clippy --workspace --all-targets --all-features -D warnings, xtask static,
  `cargo xtask affected --base origin/next --run` on e579908c (the direct tier should now select catalogs_products).
- perf-gettext-language-steady-4-plants: `cargo xtask plants ~/work/PERF-GETTEXT-LANGUAGE-STEADY-4/plants.txt --dir
  /dev/shm/claude/PERF-GETTEXT-LANGUAGE-STEADY-4/plants-1 --timeout 600`, -3's 4 plants (matches fast path, catalogs_products).
- perf-gettext-language-steady-4-list-found: wl_trace --list of gettext_found on e579908c (per-phase split for the next step).
If the gate or plants come back red, -5 fixes that first.

## Next change, written but not built (48310c65, reverted by 14f130fd: re-apply with `git revert 14f130fd`)
crates/core/src/catalog/mo.rs: `find_in_hinted` becomes an #[inline] hint test (`hinted`: whole 12-byte index head
present, the index count equals `self.n`, mode <= IX_HASH, entry < n, the indexed original same_bytes the key) plus an
#[inline(never)] `find_in_unhinted` (the old body without the hint block; `find_in` calls it). The point: hit's
`Mo::from_fields` value stays in registers on a hinted hit instead of being built in memory and passed by pointer to the
out-of-line call (-3's profile put that at ~25 instructions of `hit`'s 147). Answers are unchanged by construction (the hint
is taken under the same conditions as before; everything else runs the same search). It adds an assertion in
`same_answers`: a hint given with an index whose count (head word 1) or mode (head word 0) does not describe the image must
give the search's answer. NOT COMPILED (no local cargo; the role could not be synced while the jobs waited).
Plants for it (keke ~/work/PERF-GETTEXT-LANGUAGE-STEADY-4/plants-mo.txt, which also holds -3's 4): hint-count-ignored,
hint-mode-ignored (drop either condition of the head test), hint-unchecked (`same_bytes(...).then_some(e)` -> `Some(e)`),
no-search-after-hint (`None => self.find_in_unhinted(..)` -> `None => None`); cmd `cargo test -p liella --lib catalog::mo`.
To measure: wl_percall with `--spec validation/percall/gettext.c --base 97ae80d06b495c8ff28b265089f123ff487f6b0d`
(products cached on keke; -3's head library), WL_BASE_EXACT=1.

## Counts in force (from -3, wl_percall, Zen 1, instructions per call; glibc / -3 head)
gettext_found 455 / 558; ngettext_found 597 / 662; gettext_language_steady 466 / 684; alternating 2227 / 1283.

## Open / next step (in order)
1. Read the -4 gate, plants and trace logs (above); fix any red first.
2. Re-apply 48310c65, clippy + static on the role, gate, plants-mo.txt, percall against 97ae80d0. Keep it only if no row
   is slower.
3. Then the selector compare in `hit` (~20: `ctype_identity` match + charmap load) and translate's ~60 of key setup
   before the slot test, from the -4 trace. LANGUAGE's ~95: runs 2-4 found no safe cut without new unsafe in
   liella-boundary (RULING NEEDED + DO NOT LAND if tried).
Tool note: sync_role refuses to sync a role while jobs are queued on it (it lists their processes), even before they start.
Plan rows: saturation 14 row 6, D-7.5, D-7.12.
