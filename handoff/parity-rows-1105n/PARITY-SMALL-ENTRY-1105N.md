# PARITY-SMALL-ENTRY-1105N: ctype_b_loc, nl_langinfo and strnlen small-call entry cost, instructions:u

Id: PARITY-SMALL-ENTRY-1105N
Kind: code
Priority: 70
Size: S
Host: -
Model: opus
Effort: high
BUDGET: 70 tool calls / 140k

Counter run: label parity-rows-1105n-counts (origin/next d1a7f653); per call: label parity-rows-1105n-percall2 (probe source in this directory: small-probe.c, copy to validation/percall/small_entry.c as part of the change).

## Rows (instr ratio, time ratio)
ctype_b_loc 1.20 / 0.84; isalpha_c 1.44 / 1.00 (the percall isalpha_c is exactly 10 vs 10, so the bench row's gap is loop or table-load placement, check cbench_locale.c line ~137 first); nl_langinfo_l_en_us 1.57 / 1.62; nl_langinfo_en_us 1.13 / 1.25; strnlen_16 1.08 / 1.11; strnlen_16_mix 1.06 / 1.20.

## Profile (percall, small-probe.c, en_US.UTF-8)
ctype_b_loc 16 vs 14 (1.14); strnlen_16 32 vs 29 (1.10); nl_langinfo_mon 36 vs 34 (1.06); isalpha_c 10 vs 10. Every gap is 2-3 instructions of fixed entry cost on very small calls (the loop around them is included in the counts, so the absolute surplus is tiny); nl_langinfo_l at 1.57 is the one with a real gap and its percall case does not exist yet: add nl_langinfo_l (locale object from newlocale) to the probe first and profile it before designing.
Hypothesis: for ctype_b_loc and strnlen the surplus is a prologue/dispatch indirection (an extra jump through the dispatch slot or a stack adjustment); for nl_langinfo_l it is the locale-object category lookup path. Time ratio for ctype_b_loc is already 0.84, so rows with time below 1.0 and a gap under 3 instructions are to be recorded in the saturation register rather than chased.

## The change
Files: grep -n "ctype_b_loc\|nl_langinfo\|strnlen" crates/core/src/*.rs crates/boundary/src/*.rs csys/src/machine/*.S; read about 60 lines around the entry of each. Bring ctype_b_loc, nl_langinfo, nl_langinfo_l and strnlen (16-byte, and the mixed row) to at most 1.05x in instructions without making any other row slower (these are hot entry points: check cbench_hot too). Behaviour unchanged for every item and category (test all nl_item values in C and en_US.UTF-8, strnlen over all sizes 0-300 with every limit around the terminator). Every path meets the spec; run-time dispatch for wider paths, test both.

## JOBS (label each with the story id in lower case)
- gate: direct-tier gate (`cargo xtask affected --base origin/next --run`), clippy and `cargo xtask static` clean first.
- plants: `cargo xtask plants <file>` (docs/plants.md), one plant per changed branch, each with a msg:<regex>.
- counts before/after: `cargo xtask bench --compare --counters --runs 3 cbench_hot cbench_locale` on the unchanged and the changed tree; no row of the same bench slower.
- percall: `bash validation/wl_percall.sh --base none --spec validation/percall/small_entry.c --locale en_US.UTF-8 .` before and after (instructions:u per call).
Context discipline: read only the named ranges (grep -n first); never whole large files. Profile first (rules/developer-flow.md): write where one call's instructions go before designing.
