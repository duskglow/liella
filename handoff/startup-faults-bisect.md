# STARTUP-FAULTS-BISECT: why a static process start grew 61 -> 64..66 page faults

State: branch task/startup-faults-bisect, cut from origin/next 82fe6cfd1 (no code change; this note, the report json
and the fix brief `handoff/startup-faults-bisect/STARTUP-FAULTS-RESTORE.md`). Host: mei, scratch builds in
`~/work/STARTUP-FAULTS-BISECT` (one incremental tree, one products dir per merge, the kit's coreutils printf relinked
against each with the `~/wl/recipes/printf.cmd` recipe). Jobs: sfb-round1, sfb-maps1, sfb-step2, sfb-attrib1,
sfb-round2, sfb-attrib2, sfb-round3, sfb-hottext-next2 (superseded: sfb-hottext-next, wrong package name).

## Method

One static printf exec, `printf '%s %d %x\n' a 1 1`. Randomised runs spread by 1-2 (62 63 62 ...), so every reported
count is `setarch -R` (ASLR off, the same as hot_text_products.rs), one warm-up, then 5 runs. Those are identical
every time. Each fault address comes from `perf record -e page-faults -c 1 -d`, classified by section (`readelf -SW`)
and symbol (`nm -n -S`). Text and .rodata read faults map a 64 KiB fault-around window, so text faults count
windows, not functions.

## Result

main f3b3584af **61**, next 82fe6cfd1 **64** on mei. The recount read 66 on keke at 70cb44d76 (merge 293). The keke
number and sha 70cb were not re-measured here: no role on keke, and the brief books nothing. Per section, next minus
main: .text 11 = 11 (but a different set of windows), .bss +2, .data +1, .rodata +1, .data.rel.ro -1, .lbss -1, the
32 high-address faults identical.

The count is NOT monotone over the 298 first-parent merges main..next. Sampled counts: 61 (main) 59 (m40) 60 (m45)
65 (m50) 63 (m55) 62 (m60..m70) 66 (m74, m77) 65 (m80) 58..60 (m120..m145) 57 (m150, m155) 60 (m160) 65 (m182)
63 (m200) 59 (m216) 62 (m252, m253) 65 (m280) 64 (next). Text alone swings 7..13 windows. The start-up count is
a layout lottery around a hot group that only partly holds the start-up path. Bisecting the total therefore
finds no single culprit. The components do have culprits:

1. **DL-BOOT-RELOC-7 (merge 253, 0e4811169): +2 text windows.** `runtime::start` became generic
   (`start::<false>`, mangled `_RINvNtC15liella_boundary7runtime5startKb0_E...`), so the list key
   `_RNvNtC15liella_boundary7runtime5start` and the `Option::map_or` closure key `...runtime5starts4_0E...` match
   nothing. The new `runtime::apply_irelative` has no key. On next both run from 0x553000 and 0x567000, outside the
   hot range 0x40e1c0..0x493e40. **Proof (step 2):** next with only those three list lines corrected
   (scratch, not committed) reads **60** (text 11 -> 9, .rodata 4 -> 3, .data.rel.ro 1 -> 0). That already beats main.
2. **global_locale LOCK/GLOBAL on a .bss page of their own: +2.** First touch is a read (LOCK at 0x84c360, ip in
   the lock path), which maps the zero page; then GLOBAL is written (COW). That is 2 faults for one page. It appears
   between merge 40 (e8b7fa04e, .bss 1) and merge 45 (089f1eebb, .bss 3) and stays to next. Exact merge: **merge 42, task/integrate-hconf-3 (7091efe2e)**: merge 41 reads .bss 1 (total 59), merge 42 .bss 3 (total 60), with LOCK/GLOBAL newly faulting.
3. **.data +1, .rodata +1: layout.** cpu::AVX2_ERMS_FROM is present from merge 40 at least. In next it is the first
   start-up write to page 0x846000, but the number of .data pages swings 2..6 across history with no symbol
   added. The .rodata pair read at ip 0x40ead5 straddles the 0x760000 window, the same mechanism that
   TIMER-SIGEV-THREAD-NOSPAWN-4 recorded. These belong to the fix's "start-up data on pages already dirtied" step.

Not the cause: the COLDSTART-* merges (183..216). Around them the count goes 65 -> 63 -> 59, with no new start-up
page that persists to next.

Hot-text list census (next): **84 of 655 keys match no function in libc.a** (nm text symbols through
tools/hottext/keys.py). Main had the same kind of drift: `liella_rt::start::program` (listed, line 6) and
`locale::find_cached` ran outside the group on main as well.

## Why hot_text_products.rs did not catch it

next reads startup-only **22** (bound 23), setlocale + printf **60** (bound 68), coreutils-shaped tool **62**
(bound 71), all deterministic (job sfb-hottext-next2). Each bound is "highest count seen + 2" and has only ever
been raised (TRUE 18 -> 20 -> 23, LOCALE 65 -> 68, TOOL 68 -> 71, each on a layout-only ruling). None was ever
lowered when the count fell, so slack built up: DL-BOOT-RELOC-7's +2 fitted under it. And nothing checks the
structure, either that list keys match or that start-up functions sit in the group: `hottext::group` returns
`(hot, keys.len())` and `so.rs` drops the second value.

## Decisions / rulings

- RULING NEEDED: none for this story. The fix brief recommends ratcheting the three bounds down to count + 1 and
  making a dead hot-text key a build error, with an allow-list. That changes a policy that layout-only rulings
  have used, so the owner may want to confirm it.
- No D-row: no code changed.

## Next step

Run STARTUP-FAULTS-RESTORE (`handoff/startup-faults-bisect/STARTUP-FAULTS-RESTORE.md`). Do not redo the bisect: the
per-merge counts and maps are in mei:`~/work/STARTUP-FAULTS-BISECT/` (results.tsv, results-norand.tsv,
map-<sha>.txt, bin/printf-<sha> with link maps) until ram/disk sweeps remove them.

IMPROVEMENT: `validation/wl_count.sh` measures faults with ASLR on (perf stat -r 3 median). The same binary reads
62/63 there and 61 under `setarch -R`. A fault gate on randomised runs sees noise of +-1..2, which a deterministic
`setarch -R` count would remove.
