# PARITY-SPRINTF-CHK-LS-2: sprintf / __sprintf_chk with %s and padded width (ls nlink line), instructions:u

Id: PARITY-SPRINTF-CHK-LS-2
Kind: code
Priority: 70
Size: S
Host: -
Model: opus
Effort: high
BUDGET: 70 tool calls / 140k

Counter run: label parity-rows-1105n-counts (origin/next d1a7f653); per call: label parity-rows-1105n-percall2 (validation/percall/sprintf_chk.c).

## Rows
sprintf_chk_ls_nlink 1.17 instr / 1.14 time. Percall: spr_ls 825.5 vs 714.5 (1.16), chk_ls 828.5 vs 731.5 (1.13); chk_cpc 376 vs 361 (1.04) and chk_lu 441 vs 532 (0.83) are fine.

## Profile and hypothesis
The surplus (about 100 instructions per call) is on the %s plus padded-width path only; the integer and plain-copy paths are at parity or better. handoff/perf-sprintf-chk-ls.md (read the first 40 lines) records the earlier split: boundary vsnprintf parse/render 355, ToBuffer::put 176 (4 out-of-line calls for 1-10 byte pieces), simple_conv 100, printf_text 60. That story's Out::room change already landed or is in flight: first measure what is left on d1a7f653 with validation/wl_trace.sh (case spr_ls) and attack the largest remaining frame; candidates are the per-piece put calls and the width padding done as a loop. Do not start from the earlier guesses (byte-by-byte padding, double strlen): the earlier profile ruled them out.

## The change
Files: grep -n "render_pair_s\|simple_into\|fn put\|fn room" crates/boundary/src/*.rs crates/core/src/printf*.rs and read about 100 lines around each hit. Cut the remaining per-call work on the %s / padded-width path so spr_ls and chk_ls are at most 1.05x glibc in instructions and 1.0x in time. Output bytes unchanged: compare against the current implementation over widths 0-40, left/right justification, precision cuts and multibyte strings. Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both).

## JOBS (label each with the story id in lower case)
- gate: direct-tier gate (`cargo xtask affected --base origin/next --run`), clippy and `cargo xtask static` clean first.
- plants: `cargo xtask plants <file>` (docs/plants.md), one plant per changed branch, each with a msg:<regex>.
- counts before/after: `cargo xtask bench --compare --counters --runs 3 cbench_opt` on the unchanged and the changed tree; no row of the same bench slower.
- percall: `bash validation/wl_percall.sh --base none --spec validation/percall/sprintf_chk.c --locale en_US.UTF-8 .` before and after (instructions:u per call).
Context discipline: read only the named ranges (grep -n first); never whole large files. Profile first (rules/developer-flow.md): write where one call's instructions go before designing.
