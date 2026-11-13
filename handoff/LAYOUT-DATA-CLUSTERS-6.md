# LAYOUT-DATA-CLUSTERS-6: start-up state declared in the source (RETHINK), strings probe fixed

**State (2026-11-11):** branch `task/layout-data-clusters-6` (on -5's head `293716af8`; next not merged:
LAYOUT-DATA-CLUSTERS-4B still running). Code commit `2bb5119da`. Role mei:~/w/s3 (Skylake-SP). Scratch: mei
`~/work/LAYOUT-DATA-CLUSTERS-6/` (`evidence.sh`, `analyze.py`, `check.sh`, objdump dumps and products of the
inherited head per profile). Step 0 (clippy + static on the inherited head): `ldc6-lint0` rc 0.

## Design (RETHINK; decision record still to file: no D-X id was reserved, see the report json)
Problem: the `.data1` members came from perf first-touch traces of the current layout. Traces cannot converge
(an object already in `.data1` drops out of the next trace: #4679), a profile's `--replace` pass drops the other's
keys (#4678), first-touch misses objects on faulted pages (#4602). Options weighed:
- A. `#[link_section = ".data1"]` on each static. Rejected: edition 2024 makes it `#[unsafe(link_section)]`
  (the `unsafe_code` lint), so it is allowed only in liella-boundary (liella_legacy and liella hold 8 members).
  The default GNU ld script places only an input section named exactly `.data1`. LLVM gives a zero-initialised
  static in a named section the NOBITS type, so one codegen unit could mix a NOBITS and a PROGBITS `.data1`.
- B. A macro per static that wraps it in a marker module (`mod liella_startup`, re-exported). The path is in every
  symbol and LLVM split part, it needs no unsafe, and it is profile-independent. Cost: about 50 statics in 15
  modules, every start-up symbol renamed, edge cases (statics local to a function, `#[no_mangle]` statics still
  need a by-name list). It is the follow-up if renames of declared statics prove a burden.
- C. **Adopted:** one hand-kept list, `crates/libc-so/startup-state.list`. Each line is a Rust path
  (`liella_boundary::heap::HEAP`, encoded as the v0 symbol tail `C15liella_boundary4heap4HEAP`; any crate hash,
  `.N` split or `.llvm.N` suffix matches), `symbol <abi name>` (`.data.<n>`/`.bss.<n>`) or `section <name>` (C).
  `xtask/src/coldbss.rs` moves exactly these sections. Every entry must match in every profile unless it is
  marked `fast-only`, so a renamed static is a build error. A matched section over 8 KiB is refused. Code churn
  is one file plus the matcher, there is no file-size change (the same 80 objects plus 2 bytes) and no
  rustc/LLVM limit. The perf test `init_startup_data_accesses_are_in_the_data1_cluster` only checks the list: it
  names an undeclared static start-up touches.
- Evidence (ldc6-evidence-head): the trace keys moved the same 80 objects (10769 bytes) in both profiles. The
  list declares them, plus `classed::OUTCOME` and `dispatch::FMA` (1 byte each, fast only: release LTO folds them,
  so they are marked `fast-only`), which -5's trace named outside the cluster. Dead keys left out:
  `printf::Env::text_only::C` (no writable definition) and `liella_machine_strcmp_vlim`. Also left out:
  `liella_machine_strcmp_rescans`, which is in scan.S's plain `.bss`. If the test names it, define it in
  `.data1` in scan.S, as fold.S does.
- `startup-data.keys` stays for the STARTUP-DATA-WINDOWS contract (D-X.50) and the ro keys (startup_ro.rs). Its
  rw keys no longer place anything.
- Ruling #4677 is done: page_faults.c `strings` reads its five function pointers before `f0`.

## Results (ldc6-check-1, mei, head 2bb5119da)
| profile | true | locale | tool | fork | localtime | lt+mktime | float | strings | hot_text_products |
|---|---|---|---|---|---|---|---|---|---|
| fast | 11 | 52 | 55 | 7 (bound 7) | 19 | 19 | 3 | 0 | 22 passed |
| release | 10 | 53 | 56 | 7; **8 at `data 0x3000`** | - | - | - | - | 21 passed, 1 FAILED |
Release: `init_data_clusters_hold_at_every_text_phase` fails, with fork child 8 > FORK_BOUND 7 at the `data 0x3000` pad
(7 at every other pad). -5's regen-3 set showed the same and it is unchanged here, because the release member set
is the same 80 objects. Everything else passes in both profiles: xtask unit test, clippy, static, STARTUP-DATA-WINDOWS
(release 14 written pages <= 21, 78 of 78 objects), and the perf completeness test (the declared list covers every
small static `true` touches). Fast pays +1 on locale/tool/fork against -5's 51/54/6 (`OUTCOME` and `FMA` joined
the cluster), all inside the bounds. Whether that is order or size inside the cluster is part of the mapping below.
-5 for comparison (fast, regen-3 keys): 12 / 51 / 54 / 6; fast next 672acd9aa: 16 / 54 / 57 / 7.

## Open / next step
1. If the release result above is red at the `data` pad (fork child 8 > FORK_BOUND 7, inherited from -5's
   regen-3 set), map the release fork child (perf `-e page-faults -d` of `page_faults fork`, `data 0x3000` pad).
   Fix it by ORDER inside the cluster (members the child writes on one page), not by dropping declarations.
2. Fold `init_startup_state_statics_are_in_data1` (fixed 8-suffix array) and the keys-based
   `init_written_startup_objects_are_in_the_data1_cluster` into one list-driven test: every entry defined and in
   `.data1`, `fast-only` entries present in fast (`target/liella/.profile`). Both still pass, because the list is a
   superset of what they check.
3. File the decision record (supersedes D-X.69's keying), a plants file (an entry removed: caught by the perf test
   with msg `STARTUP-DATA-KEYS`; the matcher's `ends_with` made `==`: caught by the xtask unit test), and then the
   direct-tier gate `cargo xtask affected --run --base origin/next`. Merge origin/next once 4B lands.
4. tools/hottext/regen.sh still rewrites startup-data.keys (contract input only). Say so in its header when the
   generator is next touched.
