# LIBM-CPLX-LOG-SQRT handoff: clog/csqrt (and clogf, csqrtf) quick tiers

State: branch task/libm-cplx-log-sqrt-7 (continues -6 .. -1). HEAD in handoff/reports/LIBM-CPLX-LOG-SQRT-7.json.
Run 7 role mei:~/w/s1 (Skylake-SP). Math code unchanged since 12835f458. Decisions in force: D-10.53, D-10.74,
D-10.55, D-10.25, D-X.56, HOTTEXT-PINNING, rulings #4357, #4347 (LAYOUT-JITTER), #4383, #4430.

## Run 7: merged next (after the layout work), hot-text list repaired, gate and plants

* origin/next 07a4db6ac merged (645810174). Only conflict: docs/codemap/symbols.tsv, a generated view: next's copy
  taken (the train regenerates it).
* The merge changed inlining again: `Features::detect<CSysImpl<false>>` is out of line once more, so the
  STARTUP-TEXT-GROUPED contract failed (1 start-up function outside the hot group) with run 5's pruned list. Fix
  (8cffd5240): next's list, pruned with tools/hottext/prune.py against BOTH profiles' hot-text.matched (fast via
  `cargo xtask so`, release via `LIELLA_FULL_PROFILE=1 cargo xtask so`): it drops one key only,
  `variants::each<(), intl::resolve closure>` (its body is still in `intl::translate`, already a key). The branch's
  list now differs from next by that one line. `cargo xtask so`: all 12 layout contracts hold (start-up text 2
  windows, nothing outside the hot group; data 1 ro window, 13 written pages).
* Because hot-text.list differs from next, `cargo xtask affected --run --base origin/next` escalates to FULL
  by itself (product linker input). Job libm-cplx-log-sqrt-7-gate on mei (code tree 8cffd5240): rc 0, 2047 s,
  32 suites, 11590 passed / 0 failed / 1260 ignored, conformance 127 / 0, peak 16G (page cache), oom_kill 0.
  3 SKIPPED lines, none from this branch (dlsym helper and fpu_control dynamic wait for Liella's ld.so;
  LIELLA_REGEX_CORPUS unset).
* **LAYOUT-JITTER gone:** every start-up fault bound of hot_text_products passes on the merged head
  (init_short_programs_stay_within_their_page_fault_bounds, coreutils-shaped, fork child, first float conversion,
  localtime/mktime, zero-fault clusters/calls). The red of runs 5 and 6 does not reproduce after the layout work.
* Plants: run 5's file rerun on this head (math code unchanged since run 5), job libm-cplx-log-sqrt-7-plants:
  14 CAUGHT, 0 SURVIVED, 0 WARN (handoff/LIBM-CPLX-LOG-SQRT-7-plants.txt).
* Lint on the final code tree: job libm-cplx-log-sqrt-7-lint (clippy --all-features -D warnings, xtask clippy,
  xtask static): rc 0.
* No counts rerun: no math code changed since run 5's counts (numbers under "Earlier runs").

## Follow-up story to cut: cexp routed operand (#4432)

cbench_libm's cexp row: 1 of its 256 operands enters the 384-bit route (~115k instructions), +449 instructions per
call averaged over the row (41% of it). Not fixed here (out of this story). Direction: a ~2^-100 second tier that
decides such cases before the 384-bit route; separately, a cheaper non-FMA Dekker product (the baseline build costs
+244 per call over FMA). Data below ("cexp 5.34x on Ivy Bridge").

## Run 6 (keke:~/w/r5, Zen 1)

### Run 6: start-up fault attribution (ruling #4383)

Fast products, page_faults.c linked as hot_text_products.rs links it, relay harness (lowest of 6), 19 `pre` and 19
`cold` pads (the LAYOUT-STABILITY-RETHINK measure tool, trimmed: no layout-check; every perf fault address named by
its nearest symbol). Jobs on keke: libm-cplx-log-sqrt-6-build-next, -attr (branch build; then the build with next's
unmodified hot-text list), -measure, -regen. Counts true / locale / tool / fork, bounds 17 / 55 / 58 / 7:

| products | pad 0 | next's range over the 38 pads |
|---|---|---|
| origin/next 0d0d82e5d | 16 / 52 / 55 / 7 | true 16-17, locale 52-55, tool 55-58, fork 6-8 |
| branch c767dc493 (pruned list) | 16 / 55 / 58 / 8 | (branch: locale 54-58, tool 57-60, fork 6-9) |
| branch + fast regen | 16 / 55 / 58 / 8 | (discarded, see below) |

* **The pruning is not the cause.** With next's unmodified list the branch does not build (`2 key(s) match no
  function section`): both keys are dead on the branch and live on next, and their code now sits in functions
  that are already keys: `Features::detect<CSysImpl<false>>` is inlined into `cpu::init_with` (startup key,
  0x46c -> 0x542), and the `variants::each<(), intl::resolve closure>` body (0xe9f) into `intl::translate`
  (extra key, 0x1ac9 -> 0x2c81). Repairing the keys to those functions (#4357) changes nothing: they are listed.
* **What the extra faults are** (perf `-d`, tool and locale modes, branch minus next): `.data` +2 at
  `liella_boundary::heap::DEBUG` (read then write on a page of its own; on next it shares a page that is touched
  anyway; written start-up pages 17 vs 16; its startup-data.keys line says `release` only), and `.text` +1 in
  `liella::locale::find_cached<global_locale::Files, locale::Glue>` (0x4fa bytes, out of line on the branch and
  outside the hot group; inlined on next). Fork +1 is likewise data.
* **Why:** the branch adds code to liella_boundary (s10_math exports, hot/invtrig.rs). Generic instances of other
  crates are compiled there (`...EC15liella_boundary` keys), so the code-unit split moves. That changes inlining of
  unrelated functions (the three above, `intl::translate_unresolved` 0x3b1 -> 0x1498) and the `.data` object order.
  No start-up code changed.
* **A fast regen does not fix it** (job -regen): find_cached joins the group, but the text fault moves to
  `CtypeFile::wide` / `textdomain`, DEBUG still faults twice, written pages 18, counts 16 / 55 / 58 / 8. Not committed.
* **Next reaches each count at some pad:** locale 55 and tool 58 at cold +0xb000, +0xc000, +0xd000, +0xe000;
  fork 8 at pre +0xf000 (locale 55 also at pre +0xb000). No single pad reaches all three at once. Per #4347 /
  #4383 this is LAYOUT-JITTER: the story is held for LAYOUT-TEXT-PIN / LAYOUT-DATA-CLUSTERS. On keke the branch
  sits at the locale and tool bounds and 1 over the fork bound; run 5 read 56 / 59 / 8 on the Skylake-SP host.

## cexp 5.34x on Ivy Bridge: cause (named, not fixed)

Job libm-cplx-log-sqrt-6-cexp2 (wl_percall, validation/percall/cplx_exp.c, instructions per call, glibc dynamic;
`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA` selects both libraries' non-FMA builds, the Ivy Bridge path):

| case | FMA: glibc / Liella | no FMA: glibc / Liella |
|---|---|---|
| cexp (32 typical) | 250.8 / 396.0 | 289.1 / 640.1 (2.21x) |
| cexp_bench (cbench_libm's 256) | 249.8 / 845.0 | 288.5 / 1088.9 (3.77x) |

Two causes: (1) the quick tier's baseline build costs 640 instructions, 244 more than its FMA build: the nine
Dekker products split in software without FMA. (2) In the bench set, 1 of 256 operands (the hard case of
LIBM-CPLX-EXP-TIER-2) enters the 384-bit route, ~115k instructions, +449 per call averaged (41% of the bench row).
The time ratio (5.34x) is above the instruction ratio (3.77x). The route's lower IPC on Ivy Bridge would explain
that, but it is NOT MEASURED. Fix directions: a ~2^-100 second tier for the undecided part, and a cheaper non-FMA
product (fewer splits). cexpf is at parity (0.96x by instructions without FMA).

## Earlier runs (do not redo)

Run 5: dead-key prune (12835f458), D-10.74 axis paths, 14/14 plants CAUGHT (handoff/LIBM-CPLX-LOG-SQRT-5-plants.txt),
counts and Ivy Bridge timing (Skylake-SP instr/call: csqrtf 107.0 vs glibc 127.5, csqrt 307.2 vs 146.2, clogf 275.1
vs 274.4, clog 527.0 vs 417.6; Ivy Bridge time ratios csqrt 3.41, clog 1.52, clog_axis 1.82, clogf 0.95). Also:
bound derivations in complex_fast.rs, the atan2 merge, the route-count table, the release diagnosis of run 4.

## Next steps (in order)

1. Done in run 7 (merge, so, gate, plants): see the report json for the verdicts. If a later merge of next
   changes inlining again, repair the hot-text list as run 7 did (next's list, then prune.py with both profiles'
   hot-text.matched); never hand-edit it.
2. Follow-up stories (not this branch): csqrt by time (3.41x: dependent sqrt/div chain), cexp (the routed
   operand above, #4432, and the non-FMA product), clog 1.52x, clog_axis 1.82x.
