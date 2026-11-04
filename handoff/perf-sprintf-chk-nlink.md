# PERF-SPRINTF-CHK-NLINK (WIP, NOT READY)
Branch task/perf-sprintf-chk-nlink. Trace (job perf-sprintf-chk-nlink-trace0, keke): chk_ls 893 instr/call: render_simple<false,true> 324,
boundary vsnprintf (inlined simple_into) 224, simple_conv 100, printf_text 60, vsnprintf6 49, simple_star 37, strlen 33, va_long 29.
The _chk entry is not the gap (confirms the 10-23 result).
Change: PairS plan (core printf.rs pair_s/is_pair) + render_pair_s (boundary printf.rs) writes "%s %*s " as pieces; core test pair_s_pieces_are_what_render_simple_writes.
NOT VERIFIED: the last commit's clippy/test not re-run after the saturating_sub fix; no counts after, no gate, no plants, no static.
Next: sync, cargo xtask clippy + static, test pair_s_pieces, gate, plants (pair_s conditions, fill, star sign), percall chk_ls + bench cbench_opt before/after, saturation row 3 update.
Not done: kept-plan (stream) path does not use render_pair_s.
