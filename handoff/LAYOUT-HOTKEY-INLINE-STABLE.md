# LAYOUT-HOTKEY-INLINE-STABLE (-2): handoff

State: branch `task/layout-hotkey-inline-stable-2` (base next d1b3a596c), head in the report json
(handoff/reports/LAYOUT-HOTKEY-INLINE-STABLE-2.json). Build role mei:~/w/s2. Jobs and results: section "Verification".

## What the branch does

1. **Check** (`cargo xtask static`, xtask/src/hottext.rs `check_pinned`): every Rust key of the `startup` and `fork`
   tiers of crates/libc-so/hot-text.list must name a workspace function that is `#[inline(never)]` in the source
   (free, generic, inherent or trait method), or have a line in `crates/libc-so/hot-text.unpinned` (`<key>\t<reason>`,
   ruling #4997: a ratchet, lines may only be removed). A line whose key gained a boundary or left the tiers is an
   error. Unit tests: `listed_plain_functions_must_be_inline_never`,
   `startup_keys_without_a_fixed_boundary_fail_unless_listed`.
2. **Boundaries** (choice per key, brief item 2):
   - `#[inline(never)]` (start-up/fork-only, runs once per process): `Vdso::lookup_many` (the #4986 key; -2 changed it
     from always to never, see below), `sys::vdso_fn`, `cpu::init_with`, `runtime::{start, run_fini}`,
     `process::exit`, `stdio::new_slot`, `KernelPages::map`, `exit_handlers::{Registry::register, run_all}`,
     `stdio_compat::each_stream`.
   - `#[inline(always)]`, keys dropped at the regen (release already inlined them; the code lives in a pinned keyed
     caller in both profiles): `Vdso::{parse, version_name, defined_function, version_ok}`,
     `runtime::apply_irelative`, `Features::detect`, `stdio_compat::setup_standard`, `Heap::park_others` (fork).
   - Listed in hot-text.unpinned (30 keys, a reason each): hot paths, shared generics, closures, core/alloc,
     `refresh_words` and both `run_thread_dtors`.
3. **global_locale::GLOBAL**: its placement is declared in `crates/libc-so/startup-state.list` (matches split parts
   `GLOBAL.0`), not by startup-data.keys; both trace keys (`GLOBAL` fast, `GLOBAL.0` release) are still in
   startup-data.keys after the regen. Nothing to restore; per-phase probes below.
4. **Regen** (56832561f; one per profile, merge mode, `--allow-drop`): dropped exactly the 6 always-inlined keys plus
   `with_list<flush_all closure>` (its code is now inside the pinned `each_stream<flush_all closure>` /
   `run_fini`; its unpinned line removed) and the probe-text key `TscGate::fresh<select closure>` (fast; inlined into
   the `time_compat::select` closure, symbol evidence in mei `~/logs/lhis2-regen3.log`). startup-data.keys only gains
   objects. Both profiles rebuild with the dead-key check; all 12 layout contracts hold.

## Things not to redo (evidence)

- Always-inlining `lookup_many` (the first -2 attempt, regen1/regen2 on mei): `defined_function`/`version_ok` became
  new unpinned start-up keys, and making them always-inline too moved fat-LTO inlining elsewhere: regen2 lost four
  tool-tier keys (`intl::bind`, `intl::textdomain`, `getopt::next`, `stdio_compat::close`) and killed the extra key
  `with_list<unlink closure>` in fast. Pinning `lookup_many` (one call per process in release, next's fast shape) gave
  the same small drop set as the first regen.
- Dummy code growth (scratch only, mei `~/work/LAYOUT-HOTKEY-INLINE-STABLE-2/dummy-bessel.rs`: a 2048-entry table
  function, 128 unrolled steps, always inlined into `fast32::bessel_eval` behind a guard never true at run time):
  on **next** it fails the fast products build: dead keys `liella::locale::builtin` (locale tier) and
  `nscache::real_secs` (tool tier). Neither is a start-up/fork key: this story's check does not cover those tiers.

## Verification (head 56832561f + handoff commits; all on mei, logs `~/logs/<label>.log`)

- `lhis2-lint` rc 0: `cargo clippy --workspace --all-targets --all-features -D warnings` + `cargo xtask static`
  (the new check clean on the real lists).
- `lhis2-plants` rc 0: 3 plants, 3 CAUGHT, no WARN (handoff/LAYOUT-HOTKEY-INLINE-STABLE-2-plants.txt).
- `lhis2-gate` (affected --run --base origin/next): still running at hand-back.
- `lhis2-probe-head` rc 0: products (dead-key check) + hot_text_products in both profiles. Per-phase vs next
  (`lhis2-probe-base`, same host, same script `~/work/LAYOUT-HOTKEY-INLINE-STABLE-2/probe-job.sh`): fast identical
  at every text and data phase; release identical except the **tool** probe, **53 -> 54 faults at all 10 data
  phases** (true 10, fork 5, .data1, text phases mktime/strings/rebind 0: unchanged).
- `lhis2-probe-olddata` rc 0 (diagnostic): head with next's startup-data.keys: release tool still 54, so the +1 is
  NOT the regen's 19 added data objects; it comes from the code-shape change (pins and always-inlines).
- Dummy growth: `lhis2-probe-nextdummy` (next + dummy) rc 1: fast products dead keys `locale::builtin` (locale),
  `nscache::real_secs` (tool). `lhis2-probe-headdummy` (head + dummy) rc 1: fast dead keys `iconv::load` (extra),
  `nscache::real_secs` (tool). No start-up or fork key died in either; release passes in both. So the start-up tier
  holds, but a libm growth still needs a regen for the other tiers (the coordinator's "products check passes without
  a regen" is NOT met; see RULING below).

## Exact next step

1. Find the extra release data page of the `tool` probe: tools/hottext/faultmap.py on base vs head (release, data phase
   cold 0x800), name the object, and place it (startup-state.list / data cluster) or change the code shape that moved
   it. ALWAYS FORWARD: the branch must not land at 54.
2. Then rerun hot_text_products per phase (probe-job.sh) and the gate on the final head.

## Open / RULING NEEDED

- The locale and tool tiers are as inlining-sensitive as the start-up tier was (dummy result above). Recommend a
  follow-up story extending `check_pinned` to the `locale` and `tool` tiers with its own unpinned ratchet, pinning
  `locale::builtin` and `nscache::real_secs` first (counts needed: both are reached from setlocale/NSS paths).
- Decision record: none reserved; recommend a D-X row (static check + hot-text.unpinned ratchet) via the coordinator.
