# LAYOUT-STARTUP-STATE-LOCALE: deterministic start-up state order (#4923) and setlocale's objects (#4915)

**State (2026-11-12, run -2): INCOMPLETE, do not land.** Branch `task/layout-startup-state-locale-2` on origin/next
`dcce89422` (no merge). Role kanon:~/w/kn1. Scratch on kanon: `~/work/LAYOUT-STARTUP-STATE-LOCALE-2/` (`branch.sh`:
both profiles + checks + fault maps + pads; `faultmap.py`: perf data-fault pages per probe with the symbols on each;
`layout.py`: `.data1` per page; `pads.py` = STATE-TIME's; saved products `next-{fast,release}` (copied from run -1)
and `b1-{fast,release}` (commit 6e403c1fb)). The order now holds; **left: `true` is +1, which needs a ruling.**

## Decisions in force
Ruling #4923 (order), D-X.70 (declared list), D-X.64 (clusters), D-X.69 (access check), jitter rule #4347.
No new decision record yet (the order rule wants one: `records new decision D-NEW --series D-X`).

## Done
- Run -1 (WIP commit bcfed889c): `order_sections`/`state_order` (list order), the access check over all ten probe
  modes, the order test `init_startup_state_follows_the_declared_order`, 19 list entries (setlocale tier after
  NSCACHE; catalog tier at the end).
- Run -2 (6e403c1fb): **why the order did not survive, fixed.** (1) BFD (objcopy, ld) creates a relocation
  section's target when it reads the relocation section, so a moved section left after its relocations' slot kept
  its emitted place (`stdout`, `__tzname`): `order_sections` now moves each section's relocation sections with it,
  right after it. (2) The copies pass's `ld -r` join merged every `.data1` input in member order: the state and
  cold-bss renames now run after `direct_internal_copies` (so.rs). Unit test now reverses the emitted order with
  relocated sections at both ends and goes through objcopy + `ld -r` like the build.
- Job lssl2-branch1 (6e403c1fb): `cargo test -p xtask -- coldbss` 3 passed; hot_text_products order test + access
  checks pass in BOTH profiles; `.data1` 0x4040 bytes (align 32768).
- Pads (19 pads x 10 modes, cells next-fast/branch-fast/next-release/branch-release): locale 50/48/51/49 (orig),
  51/48/52/49 (bss 0x800): flat and 2-3 lower; tool 54/51/55/52 and 55/51/56/52: flat, 3-4 lower; fork, localtime,
  float, strings, rebind, mktime equal; **true 11/12/10/11 at every pad (+1, flat; run -1 had +2).**

## Why `true` is +1 (fault maps, faultmap.py on b1-fast vs next-fast)
`true` faults `.got`, `.data1` 0x878/0x879/0x87a and the page with `.data1`'s tail + `.bss`'s head (0x87c), where the
C members' declared words (`__libc_stack_end`, `__progname*`, `environ`: linked after the Rust object) and the probe's
own `.bss` lie. On next the base tier + those ended inside the third page (~750 bytes of slack). The new tiers sit
between the Rust base tier and the C words: setlocale + catalog small objects ~0x550 bytes and `intl::RESOLVED`
4096 bytes (alone on page 0x87b). Without RESOLVED the C words would still start at ~0x87b000: the slack is gone.
So any first-call state declared after the base tier costs `true` a page; reordering inside the Rust object cannot
fix it (cold first: the machine words of scan.S's object lead `.data1`, so `true` still spans 5 pages).

## RULING NEEDED (recommendation)
Where declared first-call state (setlocale, catalog, time tiers: touched by locale/tool/localtime, never by `true`)
goes. Recommend a second ordered cluster for first-call tiers outside the every-process run (for example
`.ldata.liella_state`, after `.bss` in GNU ld's default script, in list order; the access check accepts either
cluster), so `true`'s pages hold only every-process state. Alternative: shrink `intl::RESOLVED` (4 KiB) and keep
the tiers in `.data1` (still +1 here: not enough alone).

## Exact next step
After the ruling: implement it in coldbss.rs/so.rs (+ entry.S alignment if a cluster), rerun `branch.sh` (WORSE 0),
then the gate (`affected --run --base origin/next`), hot_text_products, plants (file below), clippy, static.
Proving "code-only change leaves counts identical" with the lgammaf branch is still undone.

## Jobs (kanon)
lssl2-lint0 (inherited head: clippy + static rc 0), lssl2-branch1 (above; rc 0, pads WORSE 38 = all `true`),
layout-startup-state-locale-2-plants (head 62fa8c2b9: 4 plants, 4 CAUGHT, no WARN), lssl2-lint1 (clippy + static on
62fa8c2b9: rc 0). Plants file with results: handoff/LAYOUT-STARTUP-STATE-LOCALE-plants.txt. Later commits: handoff only.
