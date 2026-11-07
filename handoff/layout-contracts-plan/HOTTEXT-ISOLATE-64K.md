# HOTTEXT-ISOLATE-64K: anchor the hot group (and its start-up tier) at 64 KiB boundaries, measured

Id: HOTTEXT-ISOLATE-64K
Kind: code + measurement
Size: M
Model: opus
Effort: high
Plan: handoff/layout-contracts-plan.md (section 2c C2/C3; proposed D-X.NEW-4). After HOTTEXT-STARTUP-COMPLETE
(tiers in the list).

## The change

Today the hot group starts wherever the sections before it end (0x40e440 on origin/next, 0x40e0c0 on the
LIBM-F64-LOG-FAMILY-WIRE-2 head of the same probe), so drift before the group re-cuts the 64 KiB windows inside it.

1. In `xtask/src/hottext.rs`, add an empty input section with `sh_addralign = 65536` whose name sorts first in GNU
   ld's `SORT(.text.sorted.*)` (check the sort with a probe link, e.g. `.text.sorted.h0`), carried by an archive
   member every program links (the start-up object). Optionally a second anchor before the first non-startup key so
   the start-up tier owns its windows. Confirm with `--gc-sections` that the anchors survive and do not keep
   anything else alive.
2. Measure on origin/next vs the change, both profiles: `hot_text_products.rs` counts, the start-up window count
   (LAYOUT-STARTUP-WINDOWS or the prototype), the static probe's and `libc.so.6`'s file size and text size, and the
   workload count (`wl_count`) rows. Keep the change only if no measured row is slower (faults or instructions) and
   the size cost is stated (expected up to 64 KiB of file padding per anchor; never touched).
3. For `libc.so.6` (our own link): decide whether a linker script fragment (`. = ALIGN(0x10000)` around the hot
   input sections, GNU ld 2.42) is needed beyond the anchors; GNU ld `--section-ordering-file` needs 2.43+.
4. Plants: anchor alignment lowered to 4096; anchor name that sorts after the group; anchor removed. The layout check
   must name the contract.
5. If kept: D-X.NEW-4 and a manifest row (HOT-GROUP-ANCHOR: group start % 65536 == 0 in the probe). If rejected:
   a NEGATIVE row in the saturation register with the numbers.
