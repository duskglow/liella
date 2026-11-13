# LAYOUT-TRACE-DEFAULT: trace-at-build is the default of every product build

State: branch `task/layout-trace-default-3` (run 3; HEAD in handoff/reports/LAYOUT-TRACE-DEFAULT-3.json), on top of
`task/layout-trace-default-2` (run 2) and `task/layout-trace-default` (run 1). Plan: `plan/layout-rethink.md` section 4, rulings #5190-#5194; LT-1 is
`handoff/LAYOUT-TRACE-AT-BUILD.md` (D-X.88). Decision: `records/pending/LAYOUT-TRACE-DEFAULT-1.toml` (D-NEW, series
D-X, amends D-X.88; CI holds the branch until the coordinator assigns its id). Plants:
`handoff/LAYOUT-TRACE-DEFAULT-plants.txt` (8). The story's work is done; nothing is left but landing.

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
hot_text_products comments now cite all four host classes.

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
* The bounds have no margin. All four gate host classes now measure exactly at them (run 3). A count above a bound
  on some other host is a finding about that host, not a reason to raise the bound.
* Next: LT-5 (delete the committed lists, the dead-key and pin checks, the hot-lists lane and the train regen step);
  `0` goes with it.
