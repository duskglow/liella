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

## ARCH-UTMP-LAYOUT-2 (continuation)
Cause of the surviving plant wrap-session: the x86_64 test used session 1<<40, whose low 32 bits are 0, so a wrapped store
read back as the expected 0. Fix (test only, code unchanged): x86_64 test now uses 0x1_0000_0007, 0x8000_0000, -0x8000_0001, i64::MAX
(must read 0) and i32 min/-1/0/max (kept); new test aarch64_session_outside_i32_survives_the_codec round-trips sessions beyond i32.
Jobs (shiki, role sh2): arch-utmp-layout-2-plants (4/4 CAUGHT, fresh copy plants-2, rc 0), arch-utmp-layout-2-lint (clippy workspace
all-targets all-features + cargo xtask static, rc 0). Gate arch-utmp-layout-2-gate rc 1 (escalated to FULL by the branch's xtask/spec
changes): 5863 passed, 1 failed: liella::hot_text_products init_first_string_calls_after_printf_take_no_text_fault ("1 page faults",
hot_text_products.rs:187); arch-utmp-layout-2-gate2 reran only that test: same failure (deterministic, rc 101). Unrelated to utmp
(no utmp code in that path); not investigated further (outside my change). Check whether it also fails on origin/next.

## ARCH-UTMP-LAYOUT-3 (continuation)
Cause of the red gate (hot_text_products init_first_string_calls_after_printf_take_no_text_fault, 1 fault): same role/host
(keke n2), same test: origin/next 0 faults (job arch-utmp-layout-3-hot-next), merge-base 00a8aa87 0 faults
(arch-utmp-layout-3-hot-base), this branch 1 fault (arch-utmp-layout-3-hot-branch, -branch2). So the branch caused it, not next.
Mechanism (nm of the linked probe): printf/memcmp/memchr sit in one 64 KiB window (0x47xxxx) but strncmp, part of scan.o's
shared `.text` chunk (not a hot section), landed at 0x480200 in the next window; the chunk's address follows the size of
the code linked before it, which the utmp changes moved. Fix (csys/src/machine/scan.S, commit 5927d6c9): strncmp gets its own
section `.text.strncmp` (the list key at hot-text.list:116), so the hot rename places it by list order. No list edit, no bound raised.
Result: hot_text_products 6 passed (job arch-utmp-layout-3-gate, first step), clippy + static ok in the same job.
Not done / NOT VERIFIED: no plants job for the scan.S change (the failing state before the edit, jobs -hot-branch/-branch2, is
the evidence that the test catches its absence); memcmp/memchr stay in shared chunks and could cross a window the same way later
(next step: give them own sections the same way, keys memcmp / memchr). The gate escalated to FULL (branch 129 commits behind
next; rebase/merge is the coordinator's); its result is whatever jobs.tsv shows for arch-utmp-layout-3-gate.

## ARCH-UTMP-LAYOUT-4 (continuation)
Merged origin/next (with the SPAWN-STRAY-FD fix) into the branch; no other change. Jobs on shiki role sh1, final head:
arch-utmp-layout-4-gate rc 0 (FULL tier, 2652 s), arch-utmp-layout-4-plants rc 0 (4/4 CAUGHT, fresh copy plants-1),
arch-utmp-layout-4-lint rc 0 (clippy workspace all-targets all-features + cargo xtask static).
Not done: memcmp/memchr own hot sections (optional hardening, see -3). Next: land.
