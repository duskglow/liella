# TIMING-IVB-MALLOC-CHAIN(-2): latest-allocation fast path in free, timed; NOT adopted, recorded, code reverted

State: branch task/timing-ivb-malloc-chain-2 (continues task/timing-ivb-malloc-chain), base next ff125cce. Role
keke:~/w/n2. Commits: 03d7c5f7 (experiment code, first run), 27b22934 (first run's timing handoff), 7fb7ba70 (revert
of the code: heap.rs and heap_and_sync.rs equal ff125cce again), then the register update and this handoff. Net
change against the base: docs/saturation/04-allocator.md, handoff/ only.

## Decisions in force

- Owner ruling #3210 (11-05): NOT adopted (the first refusal was #2826, judged by instructions). This run follows the
  coordinator note: docs only. The timing tables are now in the register row "malloc latest-allocation fast path in free"
  (docs/saturation/04-allocator.md) as its second NEGATIVE, judged by time. CPU classes only there (Ivy Bridge
  E3-1220L v2, Intel N100).
- No D-12 row (the owner decided; the register row is the record).

## The first run's timing jobs (brief item 1: "no job ... on nico / nozomi")

- Cause: the first run started its timing scripts with `~/bin/timing` directly, not through `~/bin/jobrun`, so nico and
  nozomi have no jobs.tsv row and no ~/logs/<label>.log for timing-ivb-malloc-chain-nico, -nico-wl10 or -nozomi.
  The raw output is still on the hosts (nico: ~/work/TIMING-IVB-MALLOC-CHAIN/ship/out/{probe,wl,wl10}.tsv; nozomi:
  {probe,wl}.tsv) and the scripts are in /home/claude/w/.scratch/TIMING-IVB-MALLOC-CHAIN/ (run.sh, summ.py).
- Not rerun: the owner has ruled, and the code is reverted, so there is no head left to time. A future timing run
  should wrap the command as `~/bin/jobrun <label> timing -- ~/bin/timing ...` so verify finds it.

## Not to redo

- Timing the path again; storing K_HINT only on change (negative twice); the instruction counts (first run).

## Open / next step

- None for this story: land the docs. If the owner ever wants the pair gain, the next experiment is a variant that costs
  nothing on frees of non-latest blocks (not specified yet). First profile du on Ivy Bridge for branch misses in free,
  to see where its +4.5% user cycles go (not profiled, NOT VERIFIED).

## Jobs (this run, final head)

- keke:timing-ivb-malloc-chain-2-static: `cargo xtask static` on keke:~/w/n2 (docs only; result in the hand-back and
  handoff/reports/TIMING-IVB-MALLOC-CHAIN-2.json).
