# LIBM-KERNEL-INDEX: generated one-page index of the libm fast kernels

State: branch `task/libm-kernel-index` (cut from origin/next c6e5db9d), code commit 72a87d2e; HEAD and job results in
`handoff/reports/LIBM-KERNEL-INDEX.json`. Role kanon:~/w/kn1. No plan rows, no D-row (tooling layout is a local detail),
no compat or differences row, no libm code change (counts unchanged, no counts job).

## What changed

1. `cargo xtask kernel-index [--check]` (xtask/src/kernel_index.rs) writes `crates/math/KERNELS.md` (`@generated`):
   one line per function (any visibility; outside `#[cfg(test)]` items and modules; `$`-templated macro fns skipped) in
   every `crates/math/src/fast*.rs` and every `.rs` under `crates/math/src/fast*/` except `tests.rs`; sections per file
   with its family (`f32 exp_log`, `f32 core`, `f64`, ...). Each line: `file:line`, signature (multi-line joined), doc
   tags, first sentence of the doc (160 chars max). Runs of an entry macro (`fast_fn!`, `nearest_fn!`) collapse to one
   line naming the entries; `kernels!(...)` invocations are printed as written. A new `fast64/` directory or new fast
   file is picked up with no change to the tool.
2. Doc-tag rule: a doc line starting `bound:`, `domain:` or `declines:` is printed as is; without a `bound:` line the
   entry says `bound: undocumented`. The tool never derives a bound from prose. Today 98 of 102 function lines say
   `bound: undocumented` (no kernel has a `bound:` tag yet; the 4 others are `macro`/invocation lines without the
   field). The first-sentence `doc:` usually states the bound in prose (e.g. exp_core 2^-45.16, log1p_core 6.8*2^-53).
   Undocumented: every function in fast32/{exp_log,hyperbolic,inv_trig,trig}.rs, fast32.rs, fast64.rs (incl.
   `fused`), fast_special.rs; `grep -n 'bound: undocumented' crates/math/KERNELS.md` lists them.
3. Freshness: `cargo xtask tables` also regenerates KERNELS.md and `tables --check` (static's `tables-fresh`, and the
   xtask test path) fails with `crates/math/KERNELS.md is out of date: run cargo xtask kernel-index` (or `is missing`).

## Flow rule text for the coordinator

libm stories read `crates/math/KERNELS.md` first and open kernel code only by range (`file:line` from the index,
`grep -n` for more) for the functions they change; never read fast32*/fast64/fast_special whole. After adding or
changing a kernel, run `cargo xtask kernel-index` (or `cargo xtask tables`) and commit KERNELS.md; static fails on a
stale copy. Put a `/// bound: ...` (and optionally `/// domain:` / `/// declines:`) doc line on new kernels.

## Jobs (kanon:~/w/kn1)

- `libm-kernel-index-check2`: `cargo xtask kernel-index && cargo xtask clippy && cargo xtask static`: rc 0
  (`kernel-index: up to date`, `tables: up to date (5 files)`).
- Gate: `cargo xtask affected --base origin/next --tier direct` ESCALATES TO FULL for any xtask/src change ("its
  commands are part of the gate"). Per the developer role no FULL was run; the gate job `libm-kernel-index-gate`
  ran the direct tests of the changed code: `cargo xtask static && cargo test -p xtask`: rc 0, xtask 194 passed / 0 failed,
  static clean (84.7 s).
- Plants `libm-kernel-index-plants`: `cargo xtask plants handoff/libm-kernel-index-plants.txt --dir
  /mnt/zram/claude/libm-kernel-index/plants-1` (dummy kernel added to fast32/trig.rs without regenerating; generator
  legend changed without rerun; both expect the out-of-date message): 2 CAUGHT, 0 SURVIVED, 0 WARN-RC-ONLY, 0 WARN-NO-MSG (157.7 s).

## Open / not to redo

- Adding `/// bound:` tags to the existing kernels is a separate doc story (copy the proven bound from each doc's
  prose; this tool must not infer it).
- INFORMATIONAL: xtask changes always escalate the direct tier to FULL; a direct-tier rule for xtask/src (xtask unit
  tests + static) would let tooling stories gate without a FULL run.

Next step: coordinator adds the flow rule above; optional story: `bound:` tags on the fast kernels.
