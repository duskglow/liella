# LAYOUT-DATA-CLUSTERS-4D: a test that catches 4C's surviving plant ldc4c-whole

State: branch `task/layout-data-clusters-4d` (stacked on LAYOUT-DATA-CLUSTERS-4C). The code commit is `0614309f6`;
the report commit sits on top of it. Brief: `~/state/briefs/LAYOUT-DATA-CLUSTERS-4D.md`. Decisions in force: those of 4C
(no new D-row: a test-only change).

## What the plant mutates (the brief's description corrected)
The brief places ldc4c-whole in the RELRO whole-page rounding. The plant's pattern actually changes
`crates/rt/src/start.rs::program`'s cut of the table to whole headers:
`phnum.min(words.len() / PHDR_WORDS) * PHDR_WORDS` becomes `phnum.min(words.len()) * PHDR_WORDS`. When `phnum`
is greater than the number of whole headers in `words`, the mutant's range runs past the slice, `get` returns None and
the table becomes empty, so nothing is read. The real code reads the whole headers. RELRO rounding (`relro_pages`)
is untouched by the plant.

## Why it survived, and the test
The existing assertion `program(&w[..10], 2, 0).tls == None` ("a truncated header is ignored") has a PT_LOAD as its
whole first header. Both versions return tls None, so the assertion cannot tell them apart. The new assertion in
`program_headers_give_the_tls_image_and_the_bias` cuts the PIE table (PT_PHDR, PT_TLS) inside its second header,
with phnum 2: the real code reads the whole PT_PHDR and gives bias 0x5555_0000 and tls None (the truncated TLS header
is ignored). The mutant gives bias 0.

## Reachability (INFORMATIONAL)
The start's only caller (`crates/boundary/src/runtime.rs::initial_thread`) passes `phnum * 7` words (phnum comes
from AT_PHNUM, which does not saturate on 64-bit) or an empty slice. In both cases `min(phnum, len)` and
`min(phnum, len / 7)` are equal, so the mutant is not a bug the start can reach today. The test pins `program`'s
documented contract (a public function: a truncated table still yields its whole headers) and was chosen over an
EQUIVALENT record because a later caller with a shorter slice would silently lose TLS and the bias. No start code
changed.

## Jobs (mei, role s1)
- ldc4d-lint: `cargo xtask clippy && cargo xtask static`, rc 0.
- LAYOUT-DATA-CLUSTERS-4D-plants: `handoff/LAYOUT-DATA-CLUSTERS-4D-plants.txt` (4C's three plants, ldc4c-whole's msg
  now names the new assertion): 3 CAUGHT, 0 SURVIVED, 0 WARN, rc 0.
- ldc4d-gate: `cargo xtask affected --run --base origin/next`, as the brief names it. It escalated to FULL
  because this branch stacks on the unlanded LAYOUT-TEXT-PIN / LAYOUT-DATA-CLUSTERS chain: the diff against
  origin/next is 76 files, including `entry.S` and xtask. It was still running at hand-back (PENDING JOBS); the
  dispatcher reads its row. The `start::` tests on this code passed as the plants baseline.

## Next step
None for this story. 4C's open item is closed; -5..-7 continue as planned in `handoff/LAYOUT-DATA-CLUSTERS-4B.md`.
