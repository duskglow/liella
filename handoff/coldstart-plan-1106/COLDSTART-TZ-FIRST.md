Id: COLDSTART-TZ-FIRST
Kind: code
Priority: 55
Size: M
Host: a build host for counts and profiles; the N100 timing host (booked) for time
Model: opus
Effort: high
BUDGET: 60 tool calls / 120k
Stack on: next (730f416b or later)

# First localtime_r with TZ=America/New_York

## Rows
cbench_opt/first_localtime_r_new_york: time 2.12x (N100), 1.96x (Ivy Bridge).

## Evidence
Probe "SCIL" - "SCI": Liella +37k user instructions, +108k kernel instructions, +16 page faults; glibc +12k user, +38k kernel, +4 faults (Broadwell-EP).

## Hypothesis
The first call reads and parses the TZif file. 16 faults vs 4 points at layout and allocation: the zone code and its data spread over many text pages of the static binary, and heap or table allocations for the parsed zone. Candidates: read the file into fixed storage with one read; parse the v2+ (64-bit) body only and skip the v1 body; keep the transitions as a view on the file bytes instead of a converted copy; find the rule lazily. Measure first (undiluted "L" probe, perf record by symbol, faults by symbol); the kernel instructions say how much is syscalls (count them with perf stat -e raw_syscalls:sys_enter where allowed).

## Tests and plants
Existing time conformance (localtime_r, mktime, tzset with TZif v1/v2/v3 fixtures) and the zone unit tests; add one that checks transitions before 1901 and after 2037 still come from the v2+ body and the footer rule (spec section 23, RFC 8536). Plants: read the v1 body; drop the footer rule; off-by-one in the transition search.

## Rules
No eager work at start-up or on the hot path: lazy work stays on the first-use path, and that path is still measured (failure-path-cost rule). A table that is a pure function of build-time data is built at build time as read-only data; a table derived from host data (locale archive, TZif, .mo) is derived per entry on demand or cheaper. Re-time on the N100 timing host with cargo xtask bench --stage / --staged; Liella profiles by symbol on a build host; glibc aggregate counters and timing only.

**JOBS:** gate (direct tier, --base origin/next), plants (cargo xtask plants), counts (the probe deltas above, before/after/glibc), timing (N100 staged bench of the rows) as <host>:<label>.

First line of the hand-back: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:.
