# LAYOUT-PLACEMENT-SWEEP: operand placement as a contract (bench offsets, kernel cost over page offsets)

Id: LAYOUT-PLACEMENT-SWEEP
Kind: code (tests + bench)
Size: S
Model: opus
Effort: high
Plan: handoff/layout-contracts-plan.md (inventory L15, L16; the BENCH-CASECMP-ALIGN row of the comparison table).

## The change

The strcasecmp cost depended on where its operands sat (BENCH-CASECMP-ALIGN-2: a page-aligned start or a start in the
last 32 bytes of a page took a slow path, 554-1949 instructions where 211-270 were expected), and the bench hid or
showed it depending on its buffer offsets. Product layout checks cannot see this; it is a property of the kernels and
of the bench.

1. Bench: `bench/c/cbench_hot.c` keeps its operands in one page-aligned block, 64 bytes in. Add `_Static_assert`s on
   the block's alignment and on each buffer's `offsetof` (page offset not 0, not within 64 bytes of the page end), so
   a reorder fails the bench build. Manifest row BENCH-OPERAND-OFFSETS if LAYOUT-MANIFEST has landed.
2. Counter sweep test (gate tier, instructions via the existing counter support; no timing): for the page-offset
   sensitive kernels (casecmp/ncasecmp, strcmp/strncmp, strlen/strnlen, memchr/rawmemchr, memcmp), run one fixed
   length per kernel with each operand at page offsets {0, 1, 15, 16, 31, 32, 4032, 4064, 4065, 4080, 4095} and
   the pairs of them, and fail when the worst offset costs more than K times the median (K from the measured
   origin/next spread; name it in the test). Name the spec clause or `no spec clause`.
3. Plants: revert BENCH-CASECMP-ALIGN-2's head fix in a scratch copy (the page-aligned start back to the generic
   loop) must fail the sweep; a bench buffer reordered must fail the build.
4. One-line D-row (local test decision).
