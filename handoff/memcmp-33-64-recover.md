# memcmp 33-64: early-difference pre-check (MEMCMP-33-64-RECOVER)

State: branch `task/memcmp-33-64-recover`, based on `origin/next` 125c0ccc; HEAD in `handoff/reports/MEMCMP-33-64-RECOVER.json`.
This is a docs-only change: the code keeps the 8-byte scalar pre-check on the 33-64 AVX2 path (`csys/src/machine/fold.S`,
`.Lcmp_wide`), as `next` already has it after bd02ee3a was reverted.

## Result (negative)

No 33-64 path keeps bd02ee3a's gain without the sort-C cost. The row is in `docs/saturation/03-memory-and-string-kernels.md`
(`memcmp` 33-64 AVX2 head pre-check).

* Shapes (measured): an LD_PRELOAD counting shim (exact n by first-difference index, memcmp/bcmp/__memcmpeq; it calls
  through to the real function, and the output md5 matched the plain run) on the glibc kit's `sort` with `LC_ALL=C` over
  `~/wl/work/words.txt` (1,000,000 lines) on keke, job `memcmp-33-64-recover-shapes` (rc 0). memcmp 18,674,556 calls:
  17-32 bytes 875,202; 33-64 bytes 17,799,354, of which 17,799,286 differ in bytes 0-7, 68 in bytes 8-15 and none
  are equal. No bcmp or __memcmpeq calls from sort.
* Why +160M (static count from the listing, not a counter run): with the pre-check a difference in bytes 0-7 is 13
  instructions from entry to ret (3 entry, 4 head compare, 6 bswap order); without it, 22 (the 8 vector instructions
  plus 11 to find and re-read the byte). 17.8M x 9 = 160.2M, the size of the measured rise. Dropping the pre-check saves
  4 on equal heads (17 to 13 when equal, 26-28 to 22-24 when the difference comes later): it was the gain on
  equal/late-difference shapes, not on early differences (the brief's wording has these the other way round).
* Break-even: the pre-check is cheaper whenever more than 4/13 (31%) of 33-64 calls differ in the head. sort-C:
  99.9996%. A vector-first path needs about 21 instructions for a head difference, so no ordering wins both
  shapes, and the entry sees only n.

## Not to redo

Dropping or moving the 33-64 pre-check (bd02ee3a, RC-SORTC-BISECT). Shape data above; the shim is
`keke:~/work/memcmp-33-64-recover/cmpshape.c` (scratch, not in the repo; `validation/wl_shapes_stub.c` is the repo's
general version).

## Open / next step

* INFORMATIONAL: the fold.S header says the 65-128 class's 32-byte early probe is there because "sort -C differs within
  bytes 8-31". On this input sort-C makes no memcmp call above 64 bytes and 99.9996% of its 33-64 calls differ in bytes
  0-7. Worth a re-check of that comment's basis the next time someone works on that class.
* Next step: none for 33-64. If equal-heavy 33-64 workloads matter, measure their head-difference rate first (break-even 31%).
* Plan rows: none were changed. No D-row: nothing was decided beyond keeping the code as it is.
