# LAYOUT-TRACE-DEFAULT: trace-at-build is the default of every product build

State: branch `task/layout-trace-default-4` (run 4; HEAD in handoff/reports/LAYOUT-TRACE-DEFAULT-4.json), origin/next
d01086da3 merged into run 3's head c605d4486, on top of runs 1-3 (`task/layout-trace-default{,-2,-3}`). Plan:
`plan/layout-rethink.md` section 4, rulings #5190-#5194; LT-1 is `handoff/LAYOUT-TRACE-AT-BUILD.md` (D-X.88).
Decision: `records/decisions/D-X/D-X.92.toml` (amends D-X.88; id assigned in run 3). Plants:
`handoff/LAYOUT-TRACE-DEFAULT-plants.txt` (8; run 4 changed no code, only a budget value and record text). The
story's work is done; nothing is left but landing.

## What changed

* Run 1 (code, unchanged in run 2): `Layout::parse(value, regen)` in xtask/src/hotlists.rs: unset / empty / `1` ->
  traced (the default), `ungrouped` -> the opt-out, `0` -> the committed lists (kept for regen / repair / comparisons
  until LT-5), anything else an error; regen builds read the committed lists; `regen.sh --build` builds its check
  with `=0`; `hot_text_products` checks the products carry the marker of their own trace.
* Run 2: the ratchet (6cc27b3b9). hot_text_products: TRUE 17 -> 11, LOCALE 51 -> 47, TOOL 56 -> 50, FORK 6 -> 5,
  LOCALTIME and LOCALTIME_MKTIME 20 -> 18, FLOAT 5 -> 3. layout/contracts.tsv: STARTUP-TEXT-WINDOWS 4/4 -> 2/2,
  STARTUP-DATA-WINDOWS rw 20/21 -> 11/14 (ro 1/1 stays). Each is the highest count over mei and keke, both profiles,
  no margin (brief step 4).
* Run 2: layout rule d (xtask/src/layout.rs, 85d33cf85) counts a changed pending DECISION record
  (records/pending/, id `D-...`) as a decision change. Before, the contracts ratchet with its pending record was red in
  `cargo xtask static` (and in handback-check) because rule d looked only under records/decisions/ and
  plan/07-decisions/. Pending compat / saturation records do not count. Tests: `rule_d_needs_a_decision_...` (two new
  cases), `only_a_pending_decision_record_counts_as_a_decision_change`; plants ltd-pending-*.

## Proof (brief step 3; run 1's jobs, read in run 2)

Jobs, all rc 0: keke `layout-trace-default-kproof2` (role n3, run-1 head 97c1112e7) and `-kbesl`; mei
`layout-trace-default-mbase` (cc19fd05e) and `-mbesl` (cc19fd05e + `origin/task/libm-f32-bessel-large-5`, libm-only:
`crates/math/src/{fast32,oracle32}/bessel.rs`, `special.rs`, `fast32.rs`, tests; equal-length paths). Each probe:
warm-up + 5 runs, hot_text_products twice per profile = 10 counted runs per probe and profile, every run identical.

| fast / release | true | locale | tool | fork | localtime | lt+mktime | float | text win | ro win | rw pages |
|---|---|---|---|---|---|---|---|---|---|---|
| mei base, mei besl, keke base, keke besl | 11 / 10 | 46 / 47 | 50 / 49 | 5 / 5 | 17 / 18 | 17 / 18 | 3 / 3 | 2 / 2 | 1 / 1 | 11 / 14 |

Base vs libm-merged (scratch scripts cmpout.sh / secdiff.py, not in the tree):
* Release: the four lists identical (same tree, both hosts too); libc.a byte-identical across hosts for the same tree
  (md5 equal keke vs mei, base and besl). Base vs besl: same size; every `.text` section of libc.a identical; only
  `__liella_build_tag`, symbol-table strings and switch-table label numbers (`.Lswitch.table.*.N`) differ.
* Fast: `startup-text.keys` and `startup-data.keys` identical; `hot-text.list` and `probe-text.keys` lose one
  extra-tier key in besl, `zone::localtime_r` (still in libc.a, but no longer run out of line). The libm change alone
  makes about 290 functions outside libm compile differently (138 changed bodies, 104 / 52 only in one build:
  inlining; `runtime::start` among them) and renumbers the hot-list ordinals of the C objects. The fault table does not
  move: this is the jitter the committed lists could not follow and the trace does.
* Same product sources, three builds (keke role n3 at 97c1112e7, mei copy at cc19fd05e, mei copy of the final head):
  the fast `hot-text.list` holds one extra-tier key (`Catalogs::identity`) in two of them and not in the mei base
  build; the other three fast lists and all release lists are identical. Same fault table in all. Not chased (the
  fault table is what is bounded); a candidate for LT-2 / LT-3 if fast lists must be exactly reproducible.

## Run 3: the other two gate hosts (shiki, kanon)

Script `proof3.sh` (scratch, not in the tree): run 2's proof.sh as a measurement (records each rc and goes on;
no `again` step). Default so, layout-check, hot_text_products twice per profile, each probe warm-up + 5 runs.
Tree: the inherited head bdfe810cf (the run-3 edits are comments, the pending record and handoff only).
* shiki (Xeon E5-2697 v3, Haswell-EP), role sh1: `layout-trace-default-3-shiki-proof` rc 0.
* kanon (Xeon E5-2699 v4, Broadwell-EP), RAM copy `/mnt/zram/claude/LAYOUT-TRACE-DEFAULT-3/proof-1`:
  `layout-trace-default-3-kanon-proof` rc 0.
* Every run identical on both hosts. libc.a byte-identical shiki vs kanon (fast md5 5e2c126a..., release
  8c31146f...). Kernel 6.8.0-147 on both.

Four-host table (bounded value = lowest of 5 runs; the highest single run equals it everywhere):

| fast / release | true | locale | tool | fork | localtime | lt+mktime | float | text win | ro win | rw pages |
|---|---|---|---|---|---|---|---|---|---|---|
| mei (Skylake-SP), keke (Zen 1) [run 2] | 11 / 10 | 46 / 47 | 50 / 49 | 5 / 5 | 17 / 18 | 17 / 18 | 3 / 3 | 2 / 2 | 1 / 1 | 11 / 14 |
| shiki (Haswell-EP), kanon (Broadwell-EP) [run 3] | 11 / 10 | 46 / 47 | 50 / 49 | 5 / 5 | 17 / 18 | 17 / 18 | 3 / 3 | 2 / 2 | 1 / 1 | 11 / 14 |
| highest of the four = branch bound | 11 | 47 | 50 | 5 | 18 | 18 | 3 | 2 / 2 | 1 / 1 | 11 / 14 |
| origin/next ba84cb320 bound | 17 | 51 | 56 | 6 | 20 | 20 | 5 | 4 / 4 | 1 / 1 | 20 / 21 |

So no bound or budget moves in run 3: each already equals the four-host highest, and each is at or below
next's. No host exceeds next's bound. The record (records/pending/LAYOUT-TRACE-DEFAULT-1.toml) and the
hot_text_products comments now cite all four host classes (c071677a5).

Run 3 jobs: `layout-trace-default-3-lint0` (shiki, inherited head bdfe810cf: clippy + static) rc 0. Proofs on
bdfe810cf (above) rc 0, superseded by the same proofs on the final head c071677a5 in RAM copies
(`/mnt/zram/claude/LAYOUT-TRACE-DEFAULT-3/proof-2` on each host): shiki `layout-trace-default-3-shiki-proof2`
(it ran beside the gate, load ~12) and kanon `layout-trace-default-3-kanon-proof2`, both rc 0, the same table
as above. Release libc.a byte-identical across the two hosts (md5 3245611c...). Fast libc.a differs: shiki's
`hot-text.list` holds one extra-tier key (`Catalogs::identity`) that kanon's does not (515 vs 516 keys; the
other three lists are identical). This is the same key and the same effect run 2 noted for builds of identical
sources; the fault table does not move. On the final head: `layout-trace-default-3-lint1` (xtask clippy, static,
cargo clippy --all-features -D warnings) rc 0, and `layout-trace-default-3-gate` (`cargo xtask affected --run`, shiki sh1, escalated to FULL by the
xtask change) rc 0: 11745 passed / 0 failed in 32 suites, conformance 132 / 0, peak 16G, 50 min beside the
shiki proof.

## Run 4: merged with next (DL-DLOPEN-TLS-SURPLUS-3), rw fast 11 -> 12

Why -3 went red in CI: on the train head (mei, fast) STARTUP-DATA-WINDOWS counted 12 written start-up pages
against the branch's rw-budget-fast 11. Every other contract and test passed.

Run 4 merged origin/next d01086da3 (clean) and ran run 3's proof3.sh (scratch: default so, layout-check,
hot_text_products twice per profile, warm-up + 5 runs per probe) on the merged head 9b3c46367 in RAM copies
(`/mnt/zram/claude/LAYOUT-TRACE-DEFAULT-4/proof-1`; keke `/dev/shm/claude/LAYOUT-TRACE-DEFAULT-4/proof-1`).
Jobs: mei `layout-trace-default-4-mei-proof`, keke `-keke-proof`, shiki `-shiki-proof`, kanon `-kanon-proof`.
Each ends rc 1 BY DESIGN: the fast `so` failed its layout check at the old budget of 11 (the products were still
written and measured in full). mei moved, so shiki and kanon were measured too (coordinator note). Every run
was identical. Release libc.a was byte-identical on all four hosts (md5 7467865d...). Fast libc.a was identical
on mei, shiki and kanon (35f7cf21...) and differs on keke (161e68b2...); runs 2 and 3 saw the same effect for
the fast hot-text list, and the fault table does not move.

| fast / release | true | locale | tool | fork | localtime | lt+mktime | float | text win | ro win | rw pages |
|---|---|---|---|---|---|---|---|---|---|---|
| -3 table, four hosts (run 3, base ba84cb320) | 11 / 10 | 46 / 47 | 50 / 49 | 5 / 5 | 17 / 18 | 17 / 18 | 3 / 3 | 2 / 2 | 1 / 1 | 11 / 14 |
| merged head, mei, keke, shiki, kanon (run 4) | 11 / 10 | 46 / 47 | 50 / 49 | 5 / 5 | 17 / 18 | 17 / 18 | 3 / 3 | 2 / 2 | 1 / 1 | **12** / 14 |
| branch bound (run 4) | 11 | 47 | 50 | 5 | 18 | 18 | 3 | 2 / 2 | 1 / 1 | **12** / 14 |
| origin/next d01086da3 bound | 17 | 51 | 56 | 6 | 20 | 20 | 5 | 4 / 4 | 1 / 1 | 20 / 21 |

Change (fec641f89): `layout/contracts.tsv` rw-budget-fast 11 -> 12; D-X.92 records the merged-head numbers and the
cause. Every other bound stays and still matches the measurement, and every bound is still below next's.

The rise comes from placement alone (coordinator question): the layout check lists
`liella_boundary::heap::RESERVE_MEM` (64 KiB zeroed array at 0x897c18, 0xc18 into a page) "alone" on pages
0x897000 and 0x898000, and printf's `Env::text_only::C` (0x856ef8-0x857288) "alone" on 0x856000, the page before
the other printf data. The traced data clusters could pack both: put the reserve page-aligned at the end of the
rw cluster, and put `text_only::C` next to the printf data on 0x857000. By page count that would read 10 fast (an
estimate, not measured). It is not done here (LT-2 clusters by phase).

Run 4 jobs on shiki role sh1: `layout-trace-default-4-lint0` (inherited head c605d4486: xtask clippy + static)
rc 0; on the final tree fec641f89, `layout-trace-default-4-lint1` (xtask clippy, static, cargo clippy
--all-features -D warnings) rc 0, and `layout-trace-default-4-gate` (`cargo xtask affected --run`, escalated to
FULL by the xtask change; its fast `so` runs the contracts at the new budget): see the report json for its result.

## Release check for the CI gate (brief step 5)

After the fast `cargo xtask affected --tier full --run`, in the same tree (it replaces the fast products, so last):
`LIELLA_FULL_PROFILE=1 cargo xtask so && LIELLA_FULL_PROFILE=1 cargo test -p liella --test hot_text_products`
(`so` runs layout-check's contracts itself). Time on mei: see "Run 2 jobs".

## Run 2 jobs (mei, role s1)

* `layout-trace-default-2-lint0` (inherited head 3e731bbee: clippy + static) rc 0.
* `layout-trace-default-2-lint` (6cc27b3b9) rc 1: static rule d refused the contracts ratchet with a pending record
  (fixed in 85d33cf85). `layout-trace-default-2-plants` stopped (older head).
* `layout-trace-default-2-lint2` (85d33cf85: clippy + static + `cargo test -p xtask hotlists` and `layout::tests`) rc 0.
* `layout-trace-default-2-proof` (85d33cf85, RAM copy of s1, proof.sh `again`) rc 0: fault table as above in both
  profiles, every bound holds (one fast `locale` run read 47 once under load 17-30; the test bounds the lowest of
  5 runs, 46); second release build byte-identical. Times under that load: fast `so` 1034 s, release `so` 549 s
  (after the fast build), release `so` again on the unchanged tree 250 s, hot_text_products 11-14 s. Run 1 on a
  quieter mei: release `so` 290 s, again 81 s, test 5 s. So the release check costs about 5-10 min on mei.
* Superseded by 5e9cb79d2 (the hand-back id scan read the placeholder ids in the new rule d tests as unassigned ids;
  the tests now use reserved-style ids, same cases): `layout-trace-default-2-gate` and `-plants2` (stopped; plants 1-5
  CAUGHT by then), `-proof` (rc 0 on 85d33cf85, numbers above), `-lint2`.
* Final head 5e9cb79d2: `layout-trace-default-2-lint3` rc 0 (clippy, static, xtask hotlists + layout tests);
  `layout-trace-default-2-gate2` (`cargo xtask affected --run`, escalated to FULL by the xtask change),
  `layout-trace-default-2-plants3` (8 plants, `/mnt/zram/claude/LAYOUT-TRACE-DEFAULT-2/plants-3`) and
  `layout-trace-default-2-proof2` (proof.sh `again`, RAM copy `head2`): running at hand-back.

## Open / not to redo

* Do not redo: the tracer survey (run 1: gdb 15.1 with Python on mei, keke, shiki, kanon; timing hosts build nothing),
  the code, the proof comparison above.
* The bounds have no margin. All four gate host classes measure exactly at them (run 3; again on the merged head in
  run 4). A count above a bound on some other host is a finding about that host, not a reason to raise the bound.
  Moving next (zero-margin budgets) can still red the train through placement alone (run 4's rw page); until LT-2
  packs the data clusters, the response is to measure the merged head and set the budget to that.
* This chain has reached -4 (here for next moving under a zero-margin budget, not a code defect): RUT RULE says a
  step-back review by a fresh agent comes before any -5.
* Next: LT-5 (delete the committed lists, the dead-key and pin checks, the hot-lists lane and the train regen step);
  `0` goes with it.
