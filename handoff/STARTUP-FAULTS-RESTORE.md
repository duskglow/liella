# STARTUP-FAULTS-RESTORE: static start-up back to 61 page faults, and contracts that hold it

State: branch `task/startup-faults-restore` (stacked on HOTTEXT-STARTUP-COMPLETE 0b03e0dca), code commit a465f7c35.
Role keke:~/w/n2. Jobs on a465f7c35: sfr-gate1 (clippy, static, affected --run --base origin/next, then the
hot_text_products counts with --nocapture), sfr-plants1 (handoff/STARTUP-FAULTS-RESTORE-plants.txt). Both were
pending at hand-back. Decisions in force: D-X.20, D-X.22, D-X.32, D-X.47. No D-row of its own yet: see Open.

## What changed
* **Dead keys fail the build** (`xtask/src/hottext.rs`): in a fast or release products build, every key of
  `crates/libc-so/hot-text.list` must match a function section, whatever its tier (HOTTEXT-STARTUP-COMPLETE checked
  only start-up and fork keys). The exceptions are a key whose trace names only the other profile, and a line in the
  new allow-list `crates/libc-so/hot-text.absent` (`<key>\t<profile>\t<reason>`). A stale allow line also fails:
  the key matches now, or is no longer in the list. `so` writes `target/product/<profile>/hot-text.matched`, and
  `tools/hottext/prune.py <list> <matched fast> <matched release>` drops keys that no build matches.
  It dropped 75, including the stale `runtime5start` key. 27 untraced keys that the release build inlines are
  allow-listed. The list keeps `start::<false>`, `apply_irelative` and the `map_or` closure (HSC already fixed those).
* **Start-up state on one page** (`xtask/src/coldbss.rs::move_startup_state`, the D-X.22 mechanism): the sections of
  `process::AUXV_WORDS`, `sys::SYSINFO_EHDR`, `cpu::AVX2_ERMS_{FROM,TO}` and `global_locale::{GLOBAL,LOCK}` are
  renamed to `.data1`. GNU ld places `.data1` after `.data`, next to `.bss`. `scan.S` puts `scan_level` there with
  `.section .data1`. Each listed static must be found exactly once, otherwise `so` fails. LLVM's `.0` part suffix is
  handled.
* **Bounds** (`crates/core/tests/hot_text_products.rs`): set to count + 1. TRUE 23 -> 18, LOCALE 68 -> 57,
  TOOL 71 -> 60. New `init_startup_path_lies_in_the_hot_text_range` links page_faults.c with `-Wl,-Map` and requires
  `_start`, `__libc_start_main` and `start::<false>`. If present, `apply_irelative` and `liella_rt::start::program` are
  checked too. Every one must lie inside the `.text.startup*` + `.text.sorted.h*` range.
* `tools/hottext/faultmap.py`: page faults -> section and nearest symbol (perf script -F addr,ip,sym).

## Measurements (keke, kit printf `'%s %d %x\n' a 1 1`, setarch -R, warm-up + 5, every run identical)
| build | faults | .data | .bss | .text |
|---|---|---|---|---|
| HSC head 0b03e0dca (sfr-base1) | 62 | 4 | 3 | 9 |
| + dead-key prune + state move (sfr-m2) | **61** | 3 | 3 | 9 |
| + cpu COPY_MIN/FILL_MIN/ERMS_LARGE_FROM, fold.S cmp_sse, global_locale::MAPS (sfr-m3) | 61 | 3 | 3 | 9 |

NEGATIVE (not kept): the m3 additions. The freed pages got other first writers: `liella_machine_fill_erms_from`,
`nscache::NSCACHE`, and `global_locale::COUNT` (read, then written: 2 faults). It is whack-a-mole, symbol by symbol.
hot_text_products counts at m3: startup 17, locale 56, tool 59, fork 7. The bounds come from these counts.
The final head (m2 layout) is checked by sfr-gate1, whose --nocapture tail prints the counts.
Target <= 61 met on keke (main = 61 per the bisect). Not measured: the Zen 1 count host, and validation/wl_count.sh
printf-loop/seq/wc/md5sum (the faults gate). Both are NOT VERIFIED.

## Open / next
* Write the D-row (D-NEW placeholder; coordinator to reserve): ".data1 start-up state group; dead hot-text key is
  a build error with hot-text.absent".
* The remaining .bss/.data start-up faults: a structural fix rather than a symbol list. One option is a
  `.data1` placement for every small static that start-up first-touches, generated from a trace the same way
  regen.sh does it.
* regen.sh: add the prune step after a regeneration of both profiles (today only the `so` error message names it).
* Run wl_count printf-loop/seq/wc/md5sum against main, and the Zen 1 count host.
