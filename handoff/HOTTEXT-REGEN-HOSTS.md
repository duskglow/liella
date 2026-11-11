# HOTTEXT-REGEN-HOSTS(-2): the hot-text list must not depend on which host or profile traced it

Branch `task/hottext-regen-hosts-2` (continues `task/hottext-regen-hosts`), head f87303a3d (code), on origin/next 2e33a3cb9.
Decision in force: **D-X.61** (host-class union with `<profile>@<host class>` provenance; now also one list per
profile). Keeps D-X.56 (start-up tier unmarked, ruling #4330), D-X.49, D-X.47. Plants: `handoff/HOTTEXT-REGEN-HOSTS-2-plants.txt`.

## State
- (1) Clean environment for the probes: HEAP-FIRST-USE-5's bbce0fc66 cherry-picked as the brief says (`env -i` with
  PATH, HOME and LANG only for every traced probe; the `page_faults true` trace already used it).
- (2) Host-class union with provenance: done in HOTTEXT-REGEN-HOSTS (merge.py, D-X.61).
- (3) Per-profile lists: done here. `crates/libc-so/hot-text.list` is split into `hot-text-fast.list` (544 keys) and
  `hot-text-release.list` (482 keys). `xtask/src/hottext.rs` `list_path(root, profile)`. regen.sh rewrites only its
  profile's list. `cargo xtask so` reads and dead-key-checks only that list, and refuses a line traced in the other
  profile (`own_profile_error`). `cargo xtask static` checks both lists. `startup-text.keys` stays one file: its order
  does not drive the layout.

## Why hrh-regen-keke-r1 failed (rc 1)
Its fast pass was fine. The release `--build` then failed STARTUP-TEXT-WINDOWS (5 windows, budget-release 4).
Cause: one shared list holds one layout order, and merge.py lets the newest trace's first-touch order win, so a fast
regen re-sorts the release keys. Evidence: keke:hottext-regen-hosts-2-lint0 (`LIELLA_FULL_PROFILE=1 cargo xtask so` on
the inherited head, no regen) fails the same way (5 > 4). mei:hrh-regen-mei-r2 never ran a release build (its log
holds only fast layout lines), so no release build of this list had passed anywhere. Not a toolchain difference:
rust-toolchain.toml pins 1.99.1 for every host.

## Jobs (keke, role n1)
- hottext-regen-hosts-2-lint0: inherited head. clippy and static ok, release so BROKEN (above; evidence).
- hottext-regen-hosts-2-rel (rc 1, clippy needless_borrow in so.rs) and -rel2 (rc 1, a newline in the D-X.61 body):
  both fixed. The plants run on the older head was stopped (kill-label).
- hottext-regen-hosts-2-plants2 (head 3dce6d7; f87303a changes only the records TOML): 6 CAUGHT, 0 SURVIVED, 0 WARN.
- hottext-regen-hosts-2-lint3 (f87303a): clippy, static, `cargo test -p xtask --bin xtask hottext` (10 passed), rc 0.
- hottext-regen-hosts-2-gate (f87303a): `affected --run --base origin/next`, started at hand-back.

## Not done (next step, in order)
1. Collect keke:hottext-regen-hosts-2-gate.
2. Release regen in the split world, on both host classes: on keke (Zen 1),
   `LIELLA_FULL_PROFILE=1 bash tools/hottext/regen.sh . --build` (its last step must pass STARTUP-TEXT-WINDOWS
   budget-release 4). Copy the rewritten `hot-text-release.list`, then repeat on mei (Skylake-SP) seeded with it, and
   commit the result. Until then the release pairs stay `release@?`, and the release build keeps failing the window
   budget as it did before this story. A fast regen can no longer change that list.
3. Optional: a fast regen on both classes to confirm the fast list is a fixed point (expected: the 63 unguarded `-`
   keys are kept, and no release key is in it any more).

## Things not to redo
Step 5 (`#[inline(never)]` on start-up keys) is RETRACTED (#4330). The union/provenance tests and plants 1-4 still
hold. Rejected for (3): one list with a per-profile rank column (two orders in one file), and keeping the old order on
merge (the fast order would then never follow its own trace).

## INFORMATIONAL
- keke's default `rustc` outside the tree is 1.99.0; the pin selects 1.99.1 inside it. Harmless, but a script that
  runs rustc outside the tree would see another compiler.
