# STARTUP-FAULTS-RESTORE: static start-up back to 61 page faults, and contracts that hold it

State: branch `task/startup-faults-restore-2` (continues `task/startup-faults-restore`, stacked on
HOTTEXT-STARTUP-COMPLETE, which is in next). The branch is 100+ commits behind origin/next and was not merged (the
brief allows no merge). Role mei:~/w/s1 (Skylake-SP). Decisions in force: D-X.20, D-X.22, D-X.32, D-X.47, **D-X.53**
(this story: bounds = count + at most 1, lowered when a count falls; a dead hot-text key fails `so` unless
allow-listed with a reason; start-up state words in `.data1`). Full report: this file. Plants:
`handoff/STARTUP-FAULTS-RESTORE-plants.txt`.

## What changed (run 1, a465f7c35..afca588ea)
* **Dead keys fail the build** (`xtask/src/hottext.rs`): in a fast or release products build, every key of
  `crates/libc-so/hot-text.list` must match a function section, whatever its tier. Exceptions: a key whose trace
  names only the other profile, and a line of `crates/libc-so/hot-text.absent` (`<key>\t<profile>\t<reason>`). A stale
  allow line also fails. `so` writes `target/product/<profile>/hot-text.matched`; `tools/hottext/prune.py <list>
  <matched fast> <matched release>` drops keys no build matches (75 dropped; 27 release-inlined keys allow-listed).
* **Start-up state on one page** (`xtask/src/coldbss.rs::move_startup_state`, the D-X.22 mechanism): the sections of
  `process::AUXV_WORDS`, `sys::SYSINFO_EHDR`, `cpu::AVX2_ERMS_{FROM,TO}`, `global_locale::{GLOBAL,LOCK}` go to `.data1`
  (after `.data`, next to `.bss`); `scan.S` puts `scan_level` there. Each listed static must be found exactly once.
* **Bounds** (`crates/core/tests/hot_text_products.rs`): TRUE 23 -> 18, LOCALE 68 -> 57, TOOL 71 -> 60; new
  `init_startup_path_lies_in_the_hot_text_range` (map-file check of `_start`, `__libc_start_main`, `start::<false>`,
  and `apply_irelative` / `liella_rt::start::program` when out of line, inside `.text.startup*` + `.text.sorted.h*`).
* `tools/hottext/faultmap.py`: page faults -> section and nearest symbol.

## Run 2 (STARTUP-FAULTS-RESTORE-2)
* sfr-gate2 (keke) failed only in clippy (`arithmetic_side_effects`, hot_text_products.rs `i + 1`, `a + s`): now
  `saturating_add` (3dd4c0a3a).
* D-X.53 written as `records/decisions/D-X/D-X.53.toml` in the records schema of next (this branch predates the records
  system, so `records check` runs only once merged; the handback check merges with next and runs static).
* FORK_BOUND 9 -> 8 (count 7 on both host classes; D-X.53 rule).
* Plant 4 fixed: it passed two test names to `cargo test` (rc 1 in the baseline, so run 1's plants never ran plant 4);
  the names now follow `--`. With that fixed, plant 4 (state move a no-op) SURVIVED the fault bounds on mei
  (plants2): each moved static is worth at most one fault, inside the +1 slack. New structural test
  `init_startup_state_statics_are_in_data1` (objdump -t of libc.a: each listed static defined in `.data1`); plant 4 now
  runs it (89744f282). Plants 1-3 CAUGHT in plants2.

## Measurements
Kit printf `'%s %d %x\n' a 1 1`, setarch -R, warm-up + 5, every run identical (keke, Zen 1, run 1):
| build | faults | .data | .bss | .text |
|---|---|---|---|---|
| HSC head 0b03e0dca | 62 | 4 | 3 | 9 |
| + dead-key prune + state move | **61** | 3 | 3 | 9 |
| + more statics to .data1 (m3, NOT kept) | 61 | 3 | 3 | 9 |

hot_text_products counts (fast products): startup 17, setlocale+printf 56, tool 59, fork 7, identical on keke (Zen 1,
run 1) and mei (Skylake-SP, startup-faults-restore-2-gate1).
wl_count vs main 01ff5a63c (mei, startup-faults-restore-2-wlc1, rev 53d549957), page faults main -> new:
printf-loop 1258745 -> 1240079, seq 126 -> 123, wc 264 -> 259, md5sum 146 -> 144. instructions:u and gating syscalls
within 0.01% (printf-loop syscalls 737629 -> 737661), output SAME. Target <= 61 met; both host classes measured.

NEGATIVE (keep): moving `cpu` COPY_MIN/FILL_MIN/ERMS_LARGE_FROM, `fold.S` cmp_sse and `global_locale::MAPS` to `.data1`
left 61: the freed pages got other first writers (`liella_machine_fill_erms_from`, `nscache::NSCACHE`,
`global_locale::COUNT`). A hand-kept symbol list does not converge.

## Jobs (mei)
startup-faults-restore-2-lint1 (clippy + static, inherited head + clippy fix) rc 0; -gate1 (clippy, static, affected
--run --base origin/next, hot_text_products --nocapture) rc 0 on 53d549957; -wlc1 rc 0; -plants1 rc 2 (plant 4 baseline
bug, superseded); -gate2 rc 0 and -plants2 rc 1 (plant 4 survived) on 5650511c4, superseded. Final head 89744f282
(later commits: handoff/ only): -gate3 and -plants3, PENDING at hand-back. If plants3 shows plant 4 SURVIVED, check
that `objdump -t` prints `.data1` as the section column for those statics.

## Open / next
* Structural fix for the remaining .data/.bss start-up faults: generate the `.data1` placement from a trace of
  first-touched small statics (as regen.sh does for text) instead of a symbol list.
* regen.sh: run prune.py after regenerating both profiles (today only the `so` error names it).
* LOCALTIME/LOCALTIME_MKTIME/FLOAT bounds (20/20/6) print no counts; apply the D-X.53 rule once they do.
