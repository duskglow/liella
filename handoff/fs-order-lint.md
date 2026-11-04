# fs-order check (FS-ORDER-LINT-2)
State: branch task/fs-order-lint-2, merged origin/next. Only one hit existed on next: crates/core/tests/no_proc_lane.rs:61,
read_dir(..).next().is_none() (an emptiness test, no order) = false positive. Fixed by narrowing: ORDERED in xtask/src/fs_order.rs
now includes ".next().is_none()" / ".next().is_some()"; bare .next() (first entry) is still flagged. Unit tests cover both shapes.
No pending list needed. Gate: cargo xtask static green, xtask fs_order tests 2/2, workspace clippy -D warnings green (keke n1).
Plants (3, all CAUGHT): drop exemption, exempt bare .next(), blind ORDERED check.
Next step: none. Not redone: no real order dependence found.
