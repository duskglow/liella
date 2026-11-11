# LAYOUT-DATA-CLUSTERS-7: fork child's state declared in the cluster; D-X.70 filed

**State (2026-11-11):** branch `task/layout-data-clusters-7` (on -6's head `6a26664bb`; next not merged, as the
brief allows no merge). Commits: `4de42c0b8` (fork child's state in `crates/libc-so/startup-state.list`),
`1fa4b9a2b` (FORK_BOUND 7 -> 6, record D-X.70, plants file). Role shiki:~/w/sh1 (Haswell-EP). Scratch: shiki
`~/work/LAYOUT-DATA-CLUSTERS-7/` (`map.sh`: products, padded probe, relay fork counts at every `data` pad, perf
`-e page-faults -d` of `fork`; `refs.sh`: data objects the fork child path references; `check.sh`; per-build
dirs `head/`, `next/`, `fix/` with the probe, link map, perf.txt, nm, sections).

## Decisions in force
- **D-X.70** (ruling #4694, reserved id): the `.data1` members are declared in `startup-state.list` (option C),
  superseding D-X.69's trace keying; A (`#[link_section]`) rejected, B (marker-module macro) the follow-up if
  renames become a burden. The list now also names the state every fork child reads.
- D-X.64 (clusters), D-X.50 (STARTUP-DATA-WINDOWS contract, still reads `startup-data.keys`).

## The release fork child at the `data 0x3000` pad: cause and fix
Step 0 (`ldc7-lint0`, clippy + static on the inherited head): rc 0. Probes (shiki, ADDR_NO_RANDOMIZE, relay fork,
10 runs each, every run identical):

| build | profile | pad 0 | 0x800 | 0x1000 | 0x2000 | 0x3000 |
|---|---|---|---|---|---|---|
| next b6f93ddee | release | 6 | 6 | 6 | 6 | 6 |
| next b6f93ddee | fast | 7 | 7 | 7 | 7 | 7 |
| inherited head 6a26664bb | release | 7 | 7 | 7 | 7 | **8** |
| inherited head 6a26664bb | fast | 7 | 7 | 7 | 7 | 7 |
| fix 4de42c0b8 | release | 5 | 5 | 5 | 5 | 5 |
| fix 4de42c0b8 | fast | 5 | 5 | 5 | 5 | 5 |

perf map of the second fork child (release): next: TLS, stack, 3 text windows, one data write. Inherited head adds
read faults: `fork::child_reset` *reads* (and writes only if set) `timer::WAKE`/`mqueue::SOCKET` (.data),
`log_compat::GLOBAL` (.data) and `gai_async::LOCK` and its neighbours (.bss page after the cluster). fork copies no
PTE for a `.data`/`.bss` page the parent never touched, so a read there faults. The cluster had emptied the `.data`
pages start-up used to touch. At pads 0..0x2000 GLOBAL's page happened to be touched by the parent; at 0x3000 it was
not (+1). Fix: the ~50 statics the child path references in `.data`/`.bss` (listed via `refs.sh` from the release and
fast disassembly of fork, child_reset and timer::after_fork_child; ~400 bytes, `.data1` 0x2b20 -> 0x2cb0, still 3
pages) are declared in the list (as `timer::EPOLL` already was). Start-up counts unchanged (fast data 0x3000: true 11,
tool 55, the -6 levels).

## Results on 1fa4b9a2b (fast is the gate's profile)
- `ldc7-check-1` (on 4de42c0b8): xtask coldbss tests ok; hot_text_products release 22 passed, fast 22 passed.
- `ldc7-lint-final`, `ldc7-gate` (direct tier, --base origin/next), `ldc7-plants`: see the report json / hand-back.

## Open / next step
1. If the dispatcher's jobs are green, this lands. Cross-host check of FORK_BOUND 6 (measured 5 here; -6 measured
   the fork child on the Skylake-SP host at 7 fast before this fix) is left to CI's hosts.
2. Not done (brief: nothing else): fold `init_startup_state_statics_are_in_data1` and
   `init_written_startup_objects_are_in_the_data1_cluster` into one list-driven test; say in tools/hottext/regen.sh's
   header that `startup-data.keys` is contract input only.
3. A static the fork child reads, added later outside the list, costs the child a fault only if its page is cold:
   the fork bound (6) and the `data` pads catch it as they caught this one.
