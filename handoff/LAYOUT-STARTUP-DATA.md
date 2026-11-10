# LAYOUT-STARTUP-DATA: start-up data objects in budgeted windows and pages (D-X.50)

**State (2026-11-10):** branch `task/layout-startup-data` from origin/next 21ef00b34; code head 1adb6d3f0, handoff
commits after it. Build role keke:~/w/r1. Decision **D-X.50** = `records/decisions/D-X/D-X.50.toml`. Plan:
`handoff/layout-contracts-plan.md` L5, L6, L14 and D-X.NEW-5.

## What exists
- `layout/contracts.tsv` row STARTUP-DATA-WINDOWS, kind `startup-data-windows` (`xtask/src/layout.rs`), rule
  `keys=crates/libc-so/startup-data.keys; text-keys=crates/libc-so/startup-text.keys; window=65536; page=4096;
  ro-budget-fast=3; ro-budget-release=4; rw-budget-fast=20; rw-budget-release=21`. `cargo xtask so` runs it last.
- Objects counted: the keys (named objects, build-stable) plus, at check time, everything the start-up functions'
  RIP-relative operands address (`objdump -d` of the linked probe), named or not. Kind by the section in the checked
  link: not writable -> 64 KiB windows; writable (.data, .data.rel.ro, .bss, .lbss) -> 4 KiB pages. An object larger
  than the unit counts its head only (RESERVE_MEM is 64 KiB). TLS is not counted (in a linked program a TLS symbol's
  value is a TLS-block offset, and .tbss overlaps .data's addresses).
- Keys: `tools/hottext/startup_data.sh <tree>` (`STARTUP_DATA_PRODUCTS=<dir>` for a saved copy) links the probe
  and runs `startup_data.py`: `perf record -e page-faults -c 1 -d` data addresses of `page_faults true` + operands of
  the start-up functions, resolved to named symbols; `anon.*` constants are left out (no stable name; the check-time
  operand pass covers them). `regen.sh` calls it after the start-up text trace. Union over both profiles.
- Measured on next's products (jobs lsd-measure-1..3): fast 3 windows / 20 pages, release 4 / 21; keys 73
  (fast 46, release 65). NSCACHE sits alone on its page in both profiles (the UTMP-Y2038-UNSIGNED-2 state).

## Not done (brief items)
- **Item 3, the section-rename fix, NOT evaluated** (budget). Candidate: rename the writable start-up objects' input
  sections to `.data1` (the default GNU ld script places `.data1` after `.data`, the `.rodata1` counterpart); needs
  a product build per profile, the fault counts of `hot_text_products.rs` and a check that no row is slower. A `.bss`
  object moved into `.data1` costs file bytes; .bss alone has no default-script grouping section.
- **Plant "NSCACHE's section moved back to .data order"**: needs item 3 first (NSCACHE is in `.data` today). Given
  instead: rw budget minus one, which must name NSCACHE as the object alone on its page.
- **Plant "a start-up rodata name moved out of STARTUP_NAMES"**: not run. The moved literal is counted (check-time
  operand pass, unit test `startup_function_operands_count_unnamed_and_unkeyed_objects`), but it only breaks the
  budget if it lands in a window start-up does not already touch; boundary's start-up literals already sit in
  window 0x750000 (fast), so it may cost no fault and survive. Run it with `cmd:cargo xtask so` and judge.

## Open questions
- RULING (recommend accept): writable sections count by 4 KiB page even when only read (a read of a file-backed
  .data page also faults around); the brief says "written"; section writability is the proxy.

## Next step
Land once lsd-gate-1adb6d3f0 and lsd-plants-1adb6d3f0 (keke) are green; then a continuation for item 3 and the
startup-names plant.
