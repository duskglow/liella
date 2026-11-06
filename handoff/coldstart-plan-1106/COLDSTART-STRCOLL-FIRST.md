Id: COLDSTART-STRCOLL-FIRST
Kind: code
Priority: 60
Size: M
Host: a build host for counts and profiles; the N100 timing host (booked) for time
Model: opus
Effort: high
BUDGET: 60 tool calls / 120k
Stack on: next (730f416b or later)

# First strcoll in a rules locale

## Rows
cbench_opt/first_strcoll: time 19.5x (N100), 20.9x (Ivy Bridge), sweep 1105.

## Evidence
First strcoll(names[69], names[70]) after setlocale(LC_ALL, "en_US.UTF-8"): Liella +11.8k user instructions, +36k kernel instructions, +3 page faults; glibc +0.3k user, ~0 kernel, 0 faults (Broadwell-EP, probe "SC" - "S").
Code: crates/core/src/locale/collate.rs: the first general-path compare calls Collate::warm -> fill, which computes the whole ASCII slot (lane[128], unit[256], rank[256]) before answering (STRCOLL-1, D-4.30).

## Hypothesis
The 11.8k instructions are the full-slot fill; the 3 faults are LC_COLLATE pages (tablemb / weightmb / indirect) touched by the fill for bytes this call never needs. Entries are already per-byte idempotent (SLOW until written, every filler writes the same value), so fill on demand per byte (lane/unit of the bytes met; rank/ascii_fast once all 128 are known) keeps later calls on the fast paths and makes the first call cost proportional to its input. Confirm the split first: perf record -e instructions:u and -e page-faults on a probe that does only setlocale + one strcoll (not diluted by other calls).

## Tests and plants
Existing strcoll/strxfrm conformance and the collate unit tests (ascii_lane_bytes); add a test that a first compare of two given strings fills only their bytes and gives the same order as after a full fill (spec Appendix D.1.4, D-4.18). Plants: write a wrong lane entry for an on-demand byte; mark ready before all bytes are known; skip the rank build.

## Rules
No eager work at start-up or on the hot path: lazy work stays on the first-use path, and that path is still measured (failure-path-cost rule). A table that is a pure function of build-time data is built at build time as read-only data; a table derived from host data (locale archive, TZif, .mo) is derived per entry on demand or cheaper. Re-time on the N100 timing host with cargo xtask bench --stage / --staged; Liella profiles by symbol on a build host; glibc aggregate counters and timing only.

**JOBS:** gate (direct tier, --base origin/next), plants (cargo xtask plants), counts (the probe deltas above, before/after/glibc), timing (N100 staged bench of the rows) as <host>:<label>.

First line of the hand-back: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:.
