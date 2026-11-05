# LOADTIME-ENTRY-DESIGN handoff

State: branch `task/loadtime-entry-design`, cut from `origin/next` `06cefa93`; deliverable
`docs/design/loadtime-entry.md` (design only, no product code). HEAD: see the report json / `git log -1`.

## Result

Recommendation **M4**: entries with CPU-variant builds live in fixed-size slots in one page-aligned section
(`.text.liella_classed`), each slot holding the SSE2 body. At static start-up step 4 (after `cpu::init_with`, before
`apply_irelative` and every constructor), on the AVX2 class, the start-up maps a private file-backed copy of that
page, writes the AVX2 bodies plus PC-relative fixups, makes it RX and `mremap`s it over the live page. Result: zero
per-call cost on both classes, the D-4.25 flag tests go away (-2 on Ivy Bridge too), the live text is never writable,
every failure leaves the SSE2 baseline. IFUNC + RELRO (M1) is +1 per static call on every class (excluded for `libc.a`
by the no-Ivy-rise rule; kept as fallback for `libc.so.6`); patched `jmp` (M2) is +1 on every AVX2 call and leaves a
non-executable page if the second `mprotect` is refused; RELRO pointer (M3) is +1 everywhere.

Predicted (counted from fold.S/scan.S paths, NOT measured): `memset_64` 20.3 -> 17.3 [glibc 18.3], `memset_128`
27.4 -> 22.4 [24.4], memset ladder 16/31 27 -> 25 [24]; memset 129-256, copy 65-256 and every strcmp call -2 on both
classes; compares NOT VERIFIED (D-4.52 words mix the lazy fold decision with the CPU one).

## Decisions

Proposed D-4.53 (text in the design doc, section 4); not added to the register (needs the rulings below).

## RULING NEEDED

1. Replacing a text page at start-up with a patched private copy (W^X kept, live page never writable): recommend approve.
2. About 6 system calls + one private page per process on the AVX2 class: recommend accept unless the infra story's
   start-up row measures worse; then M1 for `libc.so.6` only.
3. Optional hardening: move the remaining start-up words into RELRO (static) after the slots land: recommend a small
   follow-up.

## Not verified / gaps

* The predicted counts (no jobs run: design story). D-4.25 timed an AVX2 fill slower at 33-128 B with a test and a
  Rust hop in front of it, so the memset story has to time as well as count.
* Symbol type and `&memset` under M1 against the reference: `readelf` is not installed on this machine.
* The brief names `handoff/parity-memset-small-avx2.md`, but that file is only on `origin/task/parity-memset-small-avx2`,
  not on my base. Reading it would have needed `git show` of another agent's branch (forbidden), so I used the brief's
  summary and the story text in `handoff/parity-rows-avx2/PARITY-MEMSET-SMALL-AVX2.md`.
* `cargo xtask static` was not run (no cargo on this machine). I checked the doc by hand: no lab host names, LF line endings.
* SELinux `execmod` behaviour for M4's file-backed copy (whether a denial at step 4 is logged per process start in
  confined domains) is not probed: settle it in CLASSED-SLOT-INFRA.

## Next step

The coordinator rules on the three items above, then cuts CLASSED-SLOT-INFRA (doc section 4, story 1) first. The
kernel stories (memset, copy, strcmp, compares) stack on it.

Plan rows to read: D-4.25, D-4.14, D-4.45, D-4.52, D-X.15, D-10.50, D-1.13.
