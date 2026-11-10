# STARTUP-FAULTS-RESTORE: static process start back to 61 page faults or fewer, and a bound that holds it

Id: STARTUP-FAULTS-RESTORE
Kind: code
Priority: 99 (spin-up, before any other layout work; owner ruling 11-07)
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 90 tool calls / 160k

## Why

STARTUP-FAULTS-BISECT (handoff/startup-faults-bisect.md) measured the workload kit's static coreutils printf
(`printf '%s %d %x\n' a 1 1`, ASLR off via `setarch -R`, one warm-up run, then 5 runs, every run identical):
origin/main f3b3584af 61, origin/next 82fe6cfd1 64 (mei; the recount read 61 vs 66 on keke at 70cb44d76).
The growth is three separate things:

| what | faults | introduced by |
|---|---|---|
| `liella_boundary::runtime::start::<false>` and `runtime::apply_irelative` outside the hot text group (the list keys `_RNvNtC15liella_boundary7runtime5start` and the `map_or` closure key `...runtime5starts4_0E...` match nothing since `start` became generic; `apply_irelative` has no key) | +2 text (64 KiB fault-around windows 0x550000, 0x560000) | merge 253, task/dl-boot-reloc-7 |
| `liella_legacy::global_locale::{LOCK, GLOBAL}` on a .bss page of their own, first touched by a READ (LOCK load maps the zero page) and then a WRITE (COW): 2 faults for one page | +2 .bss | merge 42, task/integrate-hconf-3 |
| `liella_boundary::cpu::AVX2_ERMS_FROM` (.data) on a page nothing else at start-up writes | +1 .data (layout: the symbol exists before the growth; .data pages swing 2..6 across history) | moves with layout |
| .rodata: the two constants `env_find`-adjacent reads touch now straddle a 64 KiB window (0x75e000 / 0x760000) | +1 .rodata (layout lottery) | moves with every size change |
| `EXIT_HANDLERS` now within one .lbss page (was two) | -1 | (gain, keep it) |

Measured fix for the first row alone (scratch build of next with three corrected list lines): 64 -> 60.

## The change

1. **Hot-text keys.** Regenerate `crates/libc-so/hot-text.list` with `tools/hottext/regen.sh` (it traces first touch;
   do not hand-edit a generated file). The regenerated list must contain
   `_RINvNtC15liella_boundary7runtime5startKb0_EC15liella_boundary`, `_RNvNtC15liella_boundary7runtime15apply_irelative`
   and the `map_or` closure key `..._RINvNtC15liella_boundary7runtime5startKb0_Es4_0E...`. On next 84 of the list's 655
   keys match no function in libc.a (census method: `nm --defined-only libc.a` text symbols through
   `tools/hottext/keys.py`, compared with the list): find out which of them are on the start-up/printf path and fix them.
2. **Dead keys become an error.** `xtask/src/hottext.rs::group` returns `(hot, keys.len())` and `so.rs` drops the second
   value: an unmatched key is silent. Make `cargo xtask so` fail (or `cargo xtask static` fail) when a key matches no
   function section, with an explicit allow-list file for keys that are legitimately absent in some builds (each line
   with its reason). This is the contract that would have caught DL-BOOT-RELOC-7: a function made generic changes its
   mangled name and its key goes dead.
3. **Start-up data pages.** Put `global_locale::{LOCK, GLOBAL}` and `cpu::AVX2_ERMS_FROM` on pages start-up already
   dirties (with `AUXV_WORDS` / `SYSINFO_EHDR` / `liella_machine_scan_level`), by whichever mechanism the code already
   uses for start-up state (the cold-bss move, D-X.22, keeps small state words together; check whether a
   section or a `#[link_section]` grouping fits better), or make the first touch of LOCK a write so the page costs
   one fault, not two. Measure each step on its own (fault-address map, see below); keep only the ones that remove a fault.
4. **A bound that ratchets.** `crates/core/tests/hot_text_products.rs` bounds are "highest count seen + 2" and have
   only ever been RAISED (TRUE 18 -> 20 -> 23, LOCALE 65 -> 68, TOOL 68 -> 71); next reads 22 / 60 / 62 against 23 / 68 / 71,
   so DL-BOOT-RELOC-7's +2 text and the data pages fitted inside the accumulated slack. The runs are deterministic
   (HOTTEXT-VARIANCE), so: set each bound to the count after this fix plus at most 1, and add a structural check that
   does not depend on counts: in a linked static program, every function on the start-up path (at least `_start`,
   `__libc_start_main`, `runtime::start::<false>`, `runtime::apply_irelative`, `liella_rt::start::program`) lies inside
   the `.text.sorted.h*` range (read the addresses with `nm`, the range from a `-Wl,-Map`). Plants: drop the
   `apply_irelative` key (structural check catches it), rename a hot key to a stale form (dead-key error catches it).

## Measure

Build the products (`cargo xtask so`), relink the kit's printf from `~/validation/build/liella-m/coreutils/coreutils-9.12`
with the recipe in `~/wl/recipes/printf.cmd` and `LIELLA_PRODUCTS=<products dir>`, then
`setarch -R perf stat -x, -e page-faults <printf> '%s %d %x\n' a 1 1` (warm-up, then 5). For the per-page map:
`setarch -R perf record -e page-faults -c 1 -d`, `perf script -F addr,ip,sym`, then classify each address by
`readelf -SW` section and `nm -n -S` symbol (the bisect's scratch `faultmap.py` does this; rewrite it in
`tools/` if you keep it). Target: <= 61 on the bisect's host and on the Zen 1 count host, main = 61; expected
about 57 with all three rows fixed. Then `validation/wl_count.sh` printf-loop, seq, wc, md5sum vs main (faults gate).

## Not in scope

The 32 faults above 0x7f0000000000 (fresh heap arena touching pages at a 256 KiB stride, mmapped locale files,
stack, vdso) are the same in main and next; they are the next saturation row, not this story.
