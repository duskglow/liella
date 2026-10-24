<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 0: Foundations *(this scaffold, completed except where marked)*

* **Scope:**
  * workspace, lint and unsafe policy, the C syscall layer skeleton and its build;
  * test generation, the symbol manifest, spec Appendix C;
  * the policy checks (`purity`, `unsafe-audit`, `csys-audit`), and the C conformance runner.
* **Delivered:**
  * everything in §11, with all suites green and 4,789 pending tests;
  * spec Appendix C;
  * the license (MIT OR Apache-2.0 at the time; LGPL-2.1-or-later since 2026-10-22);
  * the git repository;
  * `crates/boundary/tests/csys_link.rs`, which proves that the C layer compiles, links, and
    is callable from safe Rust under the unsafe policy.
* **Remaining in phase 0 before phase 1:**
  1. ~~Confirm OD-C1~~: approved.
  2. ~~Run the OD-C3 spike~~: done; LTO adopted for release products (§9.1).
  3. The header-to-`ffi` consistency check (K5) and `cargo xtask bench` move into phase 1,
     where the first real wrappers and measurements appear.
  4. CI: a Linux container running `cargo xtask test` and clippy with `-D warnings`, when a
     remote exists.
* **Exit:** met.
