# ARCH-UTMP-LAYOUT handoff

State: branch task/arch-utmp-layout, code head c834392 (later commits: handoff only).

Done: `liella::users::Utmp` holds 64-bit `session` and `time`; `UtmpShape` (X86_64 384 bytes, AARCH64 400 bytes, NATIVE by
target_arch, tagged ARCH(x86_64)) drives `Utmp::encode/decode`; `to_bytes/from_bytes` use NATIVE. x86_64 bytes unchanged.
A 32-bit record stores a session or time it cannot hold as 0 / (0,0), never wrapped (D-17.5 policy extended to session
and usec). `abi/src/pack.rs` clamp removed. `crates/core/tests/utmp_shape.rs` checks both shapes against
spec/abi/layouts.tsv and layouts-aarch64.tsv and encodes/decodes an aarch64 record on the host. docs/arch-seams.md row updated.
Public API change: legacy `accounting::{now, stamp, logout_in, wtmp_record}` time is `(i64, i64)` (was `(i64, i32)`).

Jobs (shiki, role sh1): arch-utmp-layout-cross (arch-check, rc 0), arch-utmp-layout-static2, arch-utmp-layout-gate,
arch-utmp-layout-plants (4 plants: sec offset, session wrap, signed seconds, usec width). Clippy workspace all-targets rc 0.
Superseded: arch-utmp-layout-static (rc 1: new tests lacked clause tags; fixed in c834392).

Not verified: the aarch64 pack.rs path is only cross-checked (cargo check), never run. The sizes in UtmpShape are
hand-written consts checked by a test against the generated tables, not generated themselves (core cannot depend on liella-abi).
RULING NEEDED: whether to generate UtmpShape from the tables via abi-gen (recommend: no, the test suffices).
Next: read the four job rows in shiki ~/logs/jobs.tsv; no decision rows added (no D id given).
