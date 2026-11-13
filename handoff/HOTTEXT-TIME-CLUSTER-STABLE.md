# HOTTEXT-TIME-CLUSTER-STABLE(-2, -3): handoff

State (run 3): branch `task/hottext-time-cluster-stable-3` = run 2's branch + merge of origin/next 2c917bce5 (merge
commit a2700e8b3); head in handoff/reports/HOTTEXT-TIME-CLUSTER-STABLE-3.json. Build role kanon:~/w/kn1; run 3's
scratch on kanon: `~/work/HOTTEXT-TIME-CLUSTER-STABLE-3/` (run 2's proof.sh, measure.py, summ.py, faultsym.py;
chain.sh; m-pow, m-sincos = this head + libm-cplx-*-5).
Run 2 state: branch `task/hottext-time-cluster-stable-2` (stacks on `task/hottext-time-cluster-stable`), from next
bc146ccec; head in handoff/reports/HOTTEXT-TIME-CLUSTER-STABLE-2.json. Build role mei:~/w/s2. Scratch on mei:
`~/work/HOTTEXT-TIME-CLUSTER-STABLE-2/` (measure.py = LAYOUT-STABILITY-RETHINK's with env filters MPROFILES,
MREGIONS, MNOANCHOR, MNOLC; summ.py; faultsym.py = a mode's perf fault addresses -> nm symbol; mjob.sh, mrel.sh,
proof.sh; next-src = an origin/next 7ab1f35a7 tree; m-pow, m-sincos = next + libm-cplx-*-5 + this branch).
Decisions in force: D-X.89 and D-X.90 (were pending HOTTEXT-TIME-CLUSTER-STABLE-1, inline(always) on select's
lock-free path, and HOTTEXT-TIME-CLUSTER-STABLE-2-1, LOCPATH cache in a Cell). Rulings #5255 (item 1 dropped), #5256
(LT-1 closes the class), #5272 (merge next, keep both functions always inlined).

## Run 1 (HOTTEXT-TIME-CLUSTER-STABLE)

The libm branches' extra first-mktime fault was an out-of-line, unkeyed `TscGate::fresh` (fast profile), not a
lost cluster boundary. Fix: `#[inline(always)]` on `TscGate::fresh`, `recheck::tsc_holds`, `Entry::zone`,
`Glue::entry_at`, `tb::zone_entry_at`. Proof on kanon: mktime 0 at all 147 pads per profile on next+sincos-5 and
next+pow-5.

## Run 2: htcs-gate-2's red (init_data_clusters_hold_at_every_text_phase, tool 53 at cold 0x8000, 54 elsewhere)

Coordinator's first question: does origin/next give tool 53 at any pre/cold pad? **No.** mei (Skylake-SP), fast,
19 pads per region (`htcs2-measure-next`): next tool unpadded 54; cold 54-55 (55 at 0x4000, 0x5000, 0x9000, 0xE000,
none of the test's four pads); pre 54-56. Never 53. The branch (`htcs2-measure-branch`): cold 53-55 (53 at 0x8000,
0x9000), test red on mei too. So not start-up jitter alone: the branch made it phase-sensitive.

Cause (perf page-fault addresses, `faultsym.py`): the tool probe runs `OnceCell<Option<&[u8]>>::try_init` (core's
out-of-line `#[cold]` first fill behind `global_locale::Files::locpath`'s `get_or_init`) in cold text, unkeyed and
unpinnable (core code). On next it lay ~1.4 MB from `setlocale_locked` (cold too); the branch's inline set changed
liella_legacy's codegen-unit partition and moved it 53 KiB from it, so at cold phases 0x7800-0x9000 the two shared
one 64 KiB fault-around window (one fault fewer).

Fix (this run, 3c3511f8c): `Files::locpath` caches in `Cell<Option<Option<&[u8]>>>`; the fill is inlined, the probe no
longer runs that function. Same read-once-per-selection behaviour. Not done: narrowing the inline(always) set (it
would only reshuffle the partition; the cause is the unkeyed cold function, any liella_legacy change could move it).

| mei, Skylake-SP | fast tool unpadded | fast cold pads | fast pre pads | test's 4 cold pads | release tool |
|---|---|---|---|---|---|
| origin/next 7ab1f35a7 | 54 | 54-55 | 54-56 | 54 | 56 (cold 56-57, pre 54-56) |
| branch before (2b6c7a33c) | 54 | 53-55 | 54-55 | 54 54 53 54: red | not measured |
| branch with fix | 53 | 53-54 | 53-55 | 53 x4: pass | 56, identical to next (fill already inlined) |

mktime 0 at every pre/cold pad of every tree above (both profiles where measured). TOOL_BOUND stays 56: the release
count is 56 (one bound for both profiles), so the fast gain cannot tighten it. INFORMATIONAL: release tool sits AT
the bound on next (56/56) with a 57 at cold 0xC000.

Merge proofs (coordinator: re-prove LIBM-CPLX-POW-6 / SINCOS-6 after any change; -6 branches do not exist yet, so
-5): `htcs2-proof-pow` (e16164ecf), `htcs2-proof-sincos` (537f8d48b), both rc 0: first-mktime, lt+mktime bound,
zero-fault phases, data-phase and tool-bound tests pass in both profiles; mktime 0 at all 39 pre/cold configurations
per profile; fast tool 54 unpadded, cold 54-55, pre 54-56 (= next alone: the -1 seen on the branch's own base does
not show on next + libm, not investigated); release tool 56, cold 56-57.

## Run 3 (HOTTEXT-TIME-CLUSTER-STABLE-3): merge next, re-gate (ruling #5272)

CI's conflict was `crates/legacy/src/recheck.rs` only: next had put `#[inline(always)]` on `TscGate::fresh` too, with
its own comment. Resolved by keeping the attribute and next's comment, plus one line naming this story's fast-profile
case; `tsc_holds` stays `#[inline(always)]` (branch side, no conflict). No other change. All jobs on kanon
(Broadwell-EP), head a2700e8b3:

* `htcs3-lint0` (inherited head 90580ef28, step 0) and `htcs3-lint1` (merged head): clippy + static, rc 0.
* `htcs3-gate`: direct tier, 9 suites, 10035 passed, 0 failed; conformance 21 passed; peak 15G, rc 0.
* `htcs3-plants`: run 2's plants file re-run on the merged head: 3 CAUGHT, 0 SURVIVED, 2 EQUIVALENT, no WARN
  (handoff/HOTTEXT-TIME-CLUSTER-STABLE-3-plants.txt).
* `htcs3-proof-pow`, `htcs3-proof-sincos`: this head + libm-cplx-pow-5 / sincos-5 (both merged cleanly), both
  profiles: the five tests (first mktime, lt+mktime bound, zero-fault phases, data phases, coreutils bound) pass;
  mktime after localtime_r 0 faults at all 39 configurations (unpadded, 19 pre, 19 cold pads) per profile per tree.
  Data-phase test's four cold pads: fast tool 54 x4, release 56 x4. Tool over all pads: fast 54-56 (pre 54-56, cold
  pow 54-55, sincos 54-56 with 56 at cold 0x5000), release 54-57 (57 at cold 0xC000, as on next in run 2).
  No layout-jitter red: the coordinator's INCOMPLETE: LAYOUT-JITTER case did not arise.

## Next step

Coordinator: verify the JOBS in the report json; land; unhold LIBM-CPLX-POW-6 / SINCOS-6. The class stays open
until LT-1 (ruling #5256). Plants: handoff/HOTTEXT-TIME-CLUSTER-STABLE-3-plants.txt.
