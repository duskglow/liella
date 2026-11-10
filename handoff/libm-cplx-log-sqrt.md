# LIBM-CPLX-LOG-SQRT handoff: clog/csqrt (and clogf, csqrtf) quick tiers

State: branch task/libm-cplx-log-sqrt-6 (continues -5 .. -1). origin/next 0d0d82e5d merged in run 6 (commit
c767dc493, clean merge, includes the contracts.tsv line of #4386); code unchanged since 12835f458 otherwise. HEAD in
handoff/reports/LIBM-CPLX-LOG-SQRT-6.json. Run 6 role keke:~/w/r5 (Zen 1). Decisions in force: D-10.53, D-10.74,
D-10.55, D-10.25, D-X.56, HOTTEXT-PINNING, rulings #4357, #4347 (LAYOUT-JITTER), #4383 (attribute first).

## Run 6: start-up fault attribution (ruling #4383)

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

1. When LAYOUT-TEXT-PIN / LAYOUT-DATA-CLUSTERS land: merge origin/next, `cargo xtask so`, rerun the gate
   (`--files` = `git diff --name-only origin/next...HEAD` minus records/ and handoff/; FULL while hot-text.list
   differs from next). Rerun the plants file of run 5 on that head (math code unchanged since run 5).
2. Then: csqrt by time (3.41x: dependent sqrt/div chain), cexp (two causes above), clog 1.52x, clog_axis 1.82x.
