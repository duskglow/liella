# COLDSTART-SECOND-LATENCY(-4): second_setlocale_en_US / second_mktime_new_york time without instructions

State: branch task/coldstart-second-latency-4 (= -3's branch + this handoff). Code: 48843455 (proposal (a), TSC-gated
implicit zone check), unchanged in -4. Result of -4: COMPLETE for (a) (N100 not slower; keep); target not reached
(mktime 1.23x, setlocale 1.34x of glibc on the N100). Plan rows: D-23.6 (amended in -3), D-7.14 (amended in -3).
Plan: handoff/coldstart-plan-1106/.

## Why -3's N100 timing failed (nozomi:coldstart-second-latency-3-time-n100, rc 1 in 0.24 s)
Its stage.tar was hand-made (target/liella, bench/, xtask/, xtask-bin) without the workspace `Cargo.toml` and
`bench-stage.manifest`, so `xtask-bin` found no workspace root and listed no benches ("`cbench_opt` is not a benchmark
... Valid names: ."). -1's tree, made with `cargo xtask bench --stage`, ran `--compare --no-build` fine. Fix: both
trees staged with `cargo xtask bench --stage <tar> --only cbench_opt` (mei:s2, jobs coldstart-second-latency-4-stage-base
and -stage-head, rc 0; base = the worktree with 48843455's crates/legacy/src diff reverted, tree 25f47499; head tree
d2dadd44). Script: nozomi:~/work/COLDSTART-SECOND-LATENCY-4/timing.sh (smoke-tested on mei, ROUNDS=1: rc 0).

## N100 result (nozomi:coldstart-second-latency-4-time-n100, rc 0, 15 min)
A/B interleaved (A = base, B = head, 5 rounds of A1 B A2 B, ns/op medians):
| row | base | head | B/A | spread | verdict |
|---|---:|---:|---:|---:|---|
| second_mktime_new_york | 621.0 | 615.5 | 0.991 | 6.7% | same |
| second_setlocale_en_US | 256.0 | 249.0 | 0.973 | 10.8% | NOISY |
`--compare` on head (5 runs, glibc / Liella ns): second_mktime 492 / 606 (1.23x; -1: 485 / 630, 1.30x);
second_setlocale 185 / 248 (1.34x; -1: 184 / 261). Verdict: not slower beyond the spread -> keep (a).
The Ivy Bridge job nico:coldstart-second-latency-3-time-ivb never started (no jobs.tsv row on nico at 08:40Z); its tar
would have had the same missing-root defect. Not re-queued (the N100 decides).

In-process counters with the head probe (10 rounds per side; medians L / g, raw, N2 = empty window 217 / 193 cycles):
| window | cycles | instr | L2 hits | load stalls | icache miss | ld_head L1 miss | ifetch stalls |
|---|---:|---:|---:|---:|---:|---:|---:|
| S2 setlocale | 825 / 605 | 397 / 411 | 8 / 2 | 132 / 26 | 57 / 35 | 64 / 10 | 104 / 44 |
| M2 mktime | 1676 / 1305 | 1196 / 1524 | 17 / 8 | 211 / 65 | 107 / 75 | 123 / 31 | 96 / 103 |
mktime: Liella runs 330 fewer instructions and still loses ~140 cycles to L2-served loads (ld_head 123 vs 31): the
data side, i.e. (b). dTLB/iTLB walks 0 in both windows on this run.

## Next step (in order)
1. (b) one block per zone (coordinator-approved, internal layout): `time::Zone` (crates/core/src/time/zone.rs:118)
   holds five separate `TryVec`s (names, types, trans, leaps, rule_years), which is the "five anonymous mappings" of -2.
   Put them in one allocation (one aligned byte block with per-array offsets/lengths, read through slice accessors;
   rule_years stays lazily filled but reserved in the same block, or allocated alongside only for a DST rule), so a
   mktime touches consecutive lines. Its own commit, counts on a build host (L1D misses must drop from 54) and its own
   N100 A/B (stage both trees with `cargo xtask bench --stage`, reuse timing.sh with base = current head).
   Size: M (zone.rs is 61 KB: read by range; note `grep` here is ugrep with -I and skips zone.rs as binary: use
   `/usr/bin/grep -a`).
2. (c) setlocale, profile-only so far (proposal, no code): S2 is at parity by instructions (397 vs 411) but +6 L2 hits,
   +106 load_l2_hit stall cycles, ld_head 64 vs 10, +22 icache misses and +60 ifetch stall cycles: the second
   setlocale's code and data are cold in L1, not page walks (0 walks here; ~1 in -3's run). Proposal: (c1) put the
   second-call fast path (name compare against the current locale, return) and its data (current name, category
   table) in one hot section/line group (hottext.rs exists for this), (c2) keep the per-category current names in one
   cache-line-packed array instead of per-category objects. Measure ld_head and icache.misses per call before coding.

## Not to redo
-1/-2/-3 probes, traces and re-times; the N100 pre-change counters (-3); the event-group limit (5 raw + cycles +
instr); the staged-tree cause above.

## Report
- Commits in -4: this handoff + handoff/reports/COLDSTART-SECOND-LATENCY-4.json (no code change).
- Jobs on the final head (mei:s2): coldstart-second-latency-4-lint (clippy --workspace --all-targets --all-features,
  xtask static), coldstart-second-latency-4-gate (affected --run --base origin/next), coldstart-second-latency-4-plants
  (-3's 5 plants, `cargo xtask plants`); results in the hand-back. Timing: nozomi:coldstart-second-latency-4-time-n100.
- Superseded: nozomi:coldstart-second-latency-3-time-n100 (rc 1, stage without workspace root); mei:
  coldstart-second-latency-4-smoke (smoke test of the timing script, 1 round, not a verdict).
- Process slip: one local grep with an empty file list read stdin and hung (killed by PID).
- RULING NEEDED: none. Refusals: none.
