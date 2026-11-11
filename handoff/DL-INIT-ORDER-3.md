# DL-INIT-ORDER-3: attribution of the CI start-up fault red (LAYOUT-JITTER; branch neutral)

State: branch `task/dl-init-order-3` = `task/dl-init-order-2` 12be799cb plus this note and its report json. No code
change in this run. Decisions in force: D-27.19 (DL-INIT-ORDER), D-27.18 (DL-IMAGE-READER-3). Design, probes and the
story's own next steps: handoff/DL-INIT-ORDER.md; the ld.rs merge: handoff/DL-INIT-ORDER-2.md.

## What went red
CI train 582 (mei, this branch merged with LIBM-F64-SPECIAL-QUICK-2 on next): liella::hot_text_products read
startup only 18 (bound 17), setlocale + printf 57 (bound 55), coreutils-shaped tool 60 (bound 58).

## Attribution (mei, Skylake-SP, fast products, same test, four trees, every run deterministic: 5 equal runs)
| tree | startup only | setlocale + printf | tool | job |
|---|---|---|---|---|
| origin/next aad3df74d (git archive) | 17 | 55 | 58 | dl-init-order-3-attr-next |
| origin/next aad3df74d + this branch's files (= the merge: next changed none of them since 7d31b0621) | 17 | 55 | 58 | dl-init-order-3-attr-mrge |
| 7d31b0621 (the next this branch merged) | 17 | 54 | 57 | dl-init-order-3-attr-base |
| branch head 12be799cb (role s1) | 17 | 54 | 57 | dl-init-order-3-attr-head |
| bounds | 17 | 55 | 58 | |

* The branch adds nothing to the static programs: the tool program's `.text` (3,480,039 B) and `.rodata`
  (553,624 B) are the same size built from next and from next + branch, and libc.a holds no `liella_ld::init`
  symbol. Every file the branch changes is outside libc.a's code: `crates/boundary/src/ld.rs` and `arch/` are behind
  the `ld` feature (ld.so only), entry.S's change is in the `LIELLA_LD` branch only, `crates/ld/src/elf.rs` adds
  constants, and `crates/ld/src/init.rs` is used by ld.so only.
* origin/next on its own sits exactly at all three bounds on this Skylake-SP host (zero headroom; the coordinator's
  57/54 figures are for the Zen 1 host). The +1 (true), +2 (locale) and +2 (tool) over next that CI read come from the
  rest of the train, not from this branch. Per owner ruling #4347 this is LAYOUT-JITTER: no regen, no section moves,
  no bound change. Not done: building next + LIBM-F64-SPECIAL-QUICK-2 alone (composing other branches is the
  coordinator's job).
* Step 0: `cargo xtask clippy` and `cargo xtask static` on the inherited head, role s1: rc 0 (dl-init-order-3-lint0).

## Earlier verification of the same code (head 35f3e3238, keke role n3; unchanged since)
dl-init-order-2-gate rc 0 (direct tier 12161 passed; ld.so suites green), dl-init-order-2-plants 6/6 CAUGHT,
cbench_startup counts equal to next. Not rerun in this run: the code is unchanged and the story is held for the
layout fix anyway.

## Next step
When LAYOUT-TEXT-PIN / LAYOUT-DATA-CLUSTERS land (or next regains headroom on the Skylake-SP host), merge next into
this branch (no conflict expected: next changed none of its files), rerun the direct gate, plants
(handoff/DL-INIT-ORDER-plants.txt) and cbench_startup counts, and send it to the train.
