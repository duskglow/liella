# Liella: Implementation Plan

*A clean reimplementation of the glibc 2.39 x86_64 ABI in Rust, designed from
`glibc-functional-spec.md` and nothing else.*

| | |
|---|---|
| Input | `glibc-functional-spec.md`, including Appendix C (ABI data). It is the only source of truth. |
| Goals, in priority order | **Safe** (memory safe, no undefined behaviour, consistent and predictable errors) > **Fast** (no overhead the contract does not need) > **Scalable** (one design applied uniformly to all 3,943 symbols, with no historical artefacts carried into the design) |
| Method | Tests are written from the spec before any implementation code. A function is *implemented* when its tests pass, and *complete* when every edge case and failure clause of its spec entry is covered by an active test. |
| Artifacts | (1) the **spec**; (2) the **C syscall layer**, `csys/`; (3) the **Rust library**, `crates/`; (4) the **POSIX translation layer**, `crates/boundary/src/posix`. |
| Unsafe | **No `unsafe` Rust outside one boundary crate**, and inside it only in four documented categories (§2.1). Everything that needs the machine lives in the C syscall layer. |
| License | LGPL-2.1-or-later (owner, 2026-10-22) |
| Scaffold | A Cargo workspace of 10 crates plus `xtask`, and the C layer, with 4,789 generated pending tests (§4.1). It builds and runs today (§11). |

---

## How to read this plan (index)

The plan is split into part files under `plan/` (2026-10-05, owner: token cost). **Read only the parts
your task needs; never read all of them.** Section numbers (§1 to §11) and row IDs (D-…, OD-…, P…, S…) are
unchanged, so existing citations stay valid: find a row with `grep -rn "^| D-12.18 " plan/`. New rows go into
the part file that holds their section. `cargo xtask stubs` resolves D-IDs across all parts.

| § | Part file | Contents |
|---|---|---|
| 1 | [`plan/01-ground-rules.md`](plan/01-ground-rules.md) | 1. Ground rules |
| 2 | [`plan/02-design-rules.md`](plan/02-design-rules.md) | 2. Design rules (normative) |
| 3 | [`plan/03-architecture.md`](plan/03-architecture.md) | 3. Architecture |
| 4 | [`plan/04-test-methodology.md`](plan/04-test-methodology.md) | 4. Test methodology |
| 5 | [`plan/05-primitives.md`](plan/05-primitives.md) | 5. Internal primitives catalogue |
| 6 | [`plan/06-phases/00-overview.md`](plan/06-phases/00-overview.md) | 6. Phases: overview and ordering |
| 6 | [`plan/06-phases/01-phase-0-foundations-this-scaffold.md`](plan/06-phases/01-phase-0-foundations-this-scaffold.md) | Phase 0: Foundations *(this scaffold, completed except where marked)* |
| 6 | [`plan/06-phases/02-phase-1-kernel-boundary.md`](plan/06-phases/02-phase-1-kernel-boundary.md) | Phase 1: Kernel boundary |
| 6 | [`plan/06-phases/03-phase-2-pure-computation-core.md`](plan/06-phases/03-phase-2-pure-computation-core.md) | Phase 2: Pure computation core |
| 6 | [`plan/06-phases/04-track-m-parallel-from-p2.md`](plan/06-phases/04-track-m-parallel-from-p2.md) | Track M (parallel from P2): Mathematics (`libm`) |
| 6 | [`plan/06-phases/05-phase-3-internal-primitives-and.md`](plan/06-phases/05-phase-3-internal-primitives-and.md) | Phase 3: Internal primitives and memory |
| 6 | [`plan/06-phases/06-phase-4-process-runtime.md`](plan/06-phases/06-phase-4-process-runtime.md) | Phase 4: Process runtime |
| 6 | [`plan/06-phases/07-phase-5-thin-kernel-surfaces.md`](plan/06-phases/07-phase-5-thin-kernel-surfaces.md) | Phase 5: Thin kernel surfaces |
| 6 | [`plan/06-phases/08-phase-6-threads-synchronisation-process.md`](plan/06-phases/08-phase-6-threads-synchronisation-process.md) | Phase 6: Threads, synchronisation, process-wide semantics |
| 6 | [`plan/06-phases/09-phase-7-locale-and-text.md`](plan/06-phases/09-phase-7-locale-and-text.md) | Phase 7: Locale and text |
| 6 | [`plan/06-phases/10-phase-8-streams-and-formatting.md`](plan/06-phases/10-phase-8-streams-and-formatting.md) | Phase 8: Streams and formatting |
| 6 | [`plan/06-phases/11-phase-9-time-conversion-and.md`](plan/06-phases/11-phase-9-time-conversion-and.md) | Phase 9: Time conversion and zones |
| 6 | [`plan/06-phases/12-phase-10-name-services-and.md`](plan/06-phases/12-phase-10-name-services-and.md) | Phase 10: Name services and databases |
| 6 | [`plan/06-phases/13-phase-11-processes-and-terminals.md`](plan/06-phases/13-phase-11-processes-and-terminals.md) | Phase 11: Processes and terminals |
| 6 | [`plan/06-phases/14-phase-12-file-system-high.md`](plan/06-phases/14-phase-12-file-system-high.md) | Phase 12: File system (high level) and patterns |
| 6 | [`plan/06-phases/15-phase-13-remaining-services.md`](plan/06-phases/15-phase-13-remaining-services.md) | Phase 13: Remaining services |
| 6 | [`plan/06-phases/16-validation-and-optimization-before-phase.md`](plan/06-phases/16-validation-and-optimization-before-phase.md) | Validation and optimization (before phase 14) |
| 6 | [`plan/06-phases/17-phase-14-dynamic-linking.md`](plan/06-phases/17-phase-14-dynamic-linking.md) | Phase 14: Dynamic linking |
| 6 | [`plan/06-phases/18-phase-15-legacy-tier-and.md`](plan/06-phases/18-phase-15-legacy-tier-and.md) | Phase 15: Legacy tier and binary-compatibility acceptance |
| 6 | [`plan/06-phases/19-security-pass-after-phase-15.md`](plan/06-phases/19-security-pass-after-phase-15.md) | Security pass (after phase 15, before release preparation) |
| 6 | [`plan/06-phases/20-post-release-ledger-and-experiments.md`](plan/06-phases/20-post-release-ledger-and-experiments.md) | Post-release ledger and experiments |
| 7 | [`plan/07-decisions/00-overview-and-cross-cutting.md`](plan/07-decisions/00-overview-and-cross-cutting.md) | 7. Decision register: reading guide, 7.1 cross-cutting (H1-H18), 7.2 intro (rows D-T) |
| 7 | [`plan/07-decisions/c0-cross-cutting-conventions.md`](plan/07-decisions/c0-cross-cutting-conventions.md) | 7.2 C0: Cross-cutting conventions (rows D-C0) |
| 7 | [`plan/07-decisions/s01-program-startup-termination-and-environment.md`](plan/07-decisions/s01-program-startup-termination-and-environment.md) | 7.2 §1: Program startup, termination and environment (rows D-1) |
| 7 | [`plan/07-decisions/s02-error-reporting-and-diagnostics.md`](plan/07-decisions/s02-error-reporting-and-diagnostics.md) | 7.2 §2: Error reporting and diagnostics (rows D-2) |
| 7 | [`plan/07-decisions/s03-memory-allocation.md`](plan/07-decisions/s03-memory-allocation.md) | 7.2 §3: Memory allocation (rows D-3) |
| 7 | [`plan/07-decisions/s04-byte-arrays-and-strings.md`](plan/07-decisions/s04-byte-arrays-and-strings.md) | 7.2 §4: Byte arrays and strings (rows D-4, D-3) |
| 7 | [`plan/07-decisions/s05-character-classification.md`](plan/07-decisions/s05-character-classification.md) | 7.2 §5: Character classification (rows D-5) |
| 7 | [`plan/07-decisions/s06-multibyte-text-wide-characters-and.md`](plan/07-decisions/s06-multibyte-text-wide-characters-and.md) | 7.2 §6: Multibyte text, wide characters and conversion (rows D-6) |
| 7 | [`plan/07-decisions/s07-locales-and-message-translation.md`](plan/07-decisions/s07-locales-and-message-translation.md) | 7.2 §7: Locales and message translation (rows D-7) |
| 7 | [`plan/07-decisions/s08-numeric-conversion-and-integer-arithmetic.md`](plan/07-decisions/s08-numeric-conversion-and-integer-arithmetic.md) | 7.2 §8: Numeric conversion and integer arithmetic (rows D-8) |
| 7 | [`plan/07-decisions/s09-random-numbers.md`](plan/07-decisions/s09-random-numbers.md) | 7.2 §9: Random numbers (rows D-9) |
| 7 | [`plan/07-decisions/s10-mathematics.md`](plan/07-decisions/s10-mathematics.md) | 7.2 §10: Mathematics (rows D-10) |
| 7 | [`plan/07-decisions/s11-low-level-i-o.md`](plan/07-decisions/s11-low-level-i-o.md) | 7.2 §11: Low-level I/O (rows D-11) |
| 7 | [`plan/07-decisions/s12-buffered-streams.md`](plan/07-decisions/s12-buffered-streams.md) | 7.2 §12: Buffered streams (rows D-12) |
| 7 | [`plan/07-decisions/s13-file-system.md`](plan/07-decisions/s13-file-system.md) | 7.2 §13: File system (rows D-13) |
| 7 | [`plan/07-decisions/s14-searching-sorting-and-patterns.md`](plan/07-decisions/s14-searching-sorting-and-patterns.md) | 7.2 §14: Searching, sorting and patterns (rows D-14) |
| 7 | [`plan/07-decisions/s15-sockets-and-network-interfaces.md`](plan/07-decisions/s15-sockets-and-network-interfaces.md) | 7.2 §15: Sockets and network interfaces (rows D-15) |
| 7 | [`plan/07-decisions/s16-name-services.md`](plan/07-decisions/s16-name-services.md) | 7.2 §16: Name services (rows D-16) |
| 7 | [`plan/07-decisions/s17-users-groups-and-credentials.md`](plan/07-decisions/s17-users-groups-and-credentials.md) | 7.2 §17: Users, groups and credentials (rows D-17) |
| 7 | [`plan/07-decisions/s18-processes.md`](plan/07-decisions/s18-processes.md) | 7.2 §18: Processes (rows D-18) |
| 7 | [`plan/07-decisions/s19-terminals.md`](plan/07-decisions/s19-terminals.md) | 7.2 §19: Terminals (rows D-19) |
| 7 | [`plan/07-decisions/s20-signals.md`](plan/07-decisions/s20-signals.md) | 7.2 §20: Signals (rows D-20) |
| 7 | [`plan/07-decisions/s21-contexts.md`](plan/07-decisions/s21-contexts.md) | 7.2 §21: Contexts (rows D-21) |
| 7 | [`plan/07-decisions/s22-threads.md`](plan/07-decisions/s22-threads.md) | 7.2 §22: Threads (rows D-22) |
| 7 | [`plan/07-decisions/s23-time.md`](plan/07-decisions/s23-time.md) | 7.2 §23: Time (rows D-23) |
| 7 | [`plan/07-decisions/s24-resources-and-system-information.md`](plan/07-decisions/s24-resources-and-system-information.md) | 7.2 §24: Resources and system information (rows D-24) |
| 7 | [`plan/07-decisions/s25-ipc.md`](plan/07-decisions/s25-ipc.md) | 7.2 §25: IPC (rows D-25) |
| 7 | [`plan/07-decisions/s26-29.md`](plan/07-decisions/s26-29.md) | 7.2 §26–§29 (rows D-26, D-27, D-28, D-29) |
| 7 | [`plan/07-decisions/x-implementation-decisions.md`](plan/07-decisions/x-implementation-decisions.md) | 7.2 Cross-cutting implementation decisions (not tied to one annotation) (rows D-X) |
| 8 | [`plan/08-posix-layer.md`](plan/08-posix-layer.md) | 8. The POSIX translation layer |
| 9 | [`plan/09-open-decisions-and-review-log.md`](plan/09-open-decisions-and-review-log.md) | 9. Open decisions: resolved and remaining |
| 10 | [`plan/10-risks.md`](plan/10-risks.md) | 10. Risks |
| 11 | [`plan/11-scaffold-and-commands.md`](plan/11-scaffold-and-commands.md) | 11. Scaffold map and commands |
