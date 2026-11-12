# LAYOUT-HOTKEY-INLINE-STABLE: handoff (checkpoint 1, NOT VERIFIED)

State: branch `task/layout-hotkey-inline-stable`, cut from next d1b3a596c; code commit a61fb95aa36b. Role shiki:~/w/sh1
(not synced with the code yet: the base job below holds it). Nothing in this run was compiled: there is no local cargo.

## Done (commit a61fb95aa36b)

1. **Check (brief item 4)**: `cargo xtask static` (xtask/src/hottext.rs `static_check` -> `check_pinned`) now resolves every
   Rust key of the `startup` and `fork` tiers of hot-text.list (`fn_site`: free, generic, inherent `M` and trait-impl `X`
   functions, via the existing v0 walker's new `name`/`owner`) to its definition (`module_source`; `inline_never` now
   also matches indented methods inside an `impl` block whose header names the self type) and requires `#[inline(never)]`,
   unless the key has a line in the new `crates/libc-so/hot-text.unpinned` (`<key>\t<reason>`). A line whose key gained a
   boundary or left those tiers is an error. Before this, only plain free functions of the fork tier were checked
   (generic/impl/closure keys skipped: that is how `Vdso::lookup_many::<5>` escaped).
   Unit tests: `listed_plain_functions_must_be_inline_never` (rewritten for `fn_site`) and
   `startup_keys_without_a_fixed_boundary_fail_unless_listed` (fixture tree). No spec clause (build tooling).
2. **Survey (brief item 1)**: of the ~54 Rust start-up keys only 5 had a fixed boundary (debug_alloc, internal_large,
   malloc_general, parts_first_use, resolve_fold_slow). Choice per key (item 2):
   - `#[inline(always)]`, keys to drop at the regen: release already inlines them (traced `fast` only), so release code
     is unchanged and fast stops flipping; the code lives in the keyed caller in both profiles: `Vdso::{parse,
     lookup_many, version_name}` (into `sys::vdso_fn`), `runtime::apply_irelative`, `Features::detect`,
     `stdio_compat::setup_standard`, fork tier `Heap::park_others`.
   - `#[inline(never)]`: start-up/fork-only functions that already have a symbol in both profiles (expected free; NOT
     MEASURED yet): `sys::vdso_fn`, `cpu::init_with`, `runtime::{start, run_fini}`, `process::exit`, `stdio::new_slot`,
     `KernelPages::map`, `exit_handlers::{Registry::register, run_all}`, `stdio_compat::each_stream`.
   - Listed in hot-text.unpinned (33 keys, reason each): hot paths (heap, conv, printf, stream, stdio_compat), shared
     generics (`stdio::with`, `with_list`, `TryBox::new`), closures, core/alloc functions, and `refresh_words` +
     both `run_thread_dtors` (boundary measured to cost start-up instructions, handoff/hottext-regen-unblock.md).
3. **global_locale::GLOBAL (item 3)**: on next it is still keyed (`GLOBAL` fast, `GLOBAL.0` release; LLVM's split part).
   Its placement does not come from startup-data.keys: it is DECLARED in `crates/libc-so/startup-state.list` line 125,
   which matches split parts and suffixes (xtask/src/coldbss.rs `STATE_LIST_FILE`, LAYOUT-DATA-CLUSTERS-6). The #4991
   regen dropping the trace key therefore did not drop its placement; nothing to restore (to be confirmed by the
   per-phase probes below).

## Exact next steps (continuation)

1. `~/bin/jobwait lhis-phases-base` on shiki (base = next, both profiles; output
   `~/work/LAYOUT-HOTKEY-INLINE-STABLE/phases-base.txt`, script `phases.sh` beside it). Then sync the branch.
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings` and `cargo test -p xtask hottext::` (fix compile
   errors: none of this was compiled).
3. Regen, one per profile in merge mode: `tools/hottext/regen.sh . --build --allow-drop`, then
   `LIELLA_FULL_PROFILE=1 tools/hottext/regen.sh . --build --allow-drop`. Check that the ONLY dropped keys are the seven
   always-inlined ones above (plus their probe-text.keys lines); anything else = stop and look. Commit the lists.
4. `cargo xtask static` (must be clean; an unpinned key the survey missed shows here), then the gate
   (`affected --run --base origin/next`), `phases.sh` on the head vs base (fast and release, every text and data phase:
   the release start-up probes must be unchanged; the inline(never) marks must not add faults or instructions), and the
   plants: drop `#[inline(never)]` from `sys::vdso_fn` (msg:`vdso_fn is not #\[inline\(never\)\]`), delete the
   `rust_alloc` line of hot-text.unpinned (msg:`rust_alloc .startup key.: .*not #\[inline\(never\)\]`), via
   `cargo xtask plants` with `cmd:cargo xtask static`.
5. Decision row: none reserved; this changes a static check and a build-input list (recommend a D-X row via the
   coordinator).

## Open / RULING NEEDED

- The 33 unpinned keys stay inlining-sensitive (as before). Recommendation: accept; the dead-key check plus
  `cargo xtask hottext-repair` stay their guard; pin more only with per-row instruction counts showing no cost.
