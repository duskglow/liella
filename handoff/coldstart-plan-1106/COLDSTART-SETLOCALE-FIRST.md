Id: COLDSTART-SETLOCALE-FIRST
Kind: code
Priority: 60
Size: M
Host: a build host for counts and profiles; the N100 timing host (booked) for time
Model: opus
Effort: high
BUDGET: 70 tool calls / 140k
Stack on: next (730f416b or later)

# First setlocale(LC_ALL, "en_US.UTF-8")

## Rows
cbench_opt/first_setlocale_en_US: time 2.22x (N100), 2.23x (Ivy Bridge).

## Evidence
Probe "S" - "": Liella +63.6k user instructions, +362k kernel instructions, +38 page faults; glibc +23k user, +134k kernel, +12 faults (Broadwell-EP).
Liella profile of the "S" run (instructions:u): locale::find_named, CtypeFile::parse, CtypeFile::ascii_case_runs, machine_copy_avx2, machine_equal, Archive::lookup, codeset_matches. Page faults: Heap::refill and Heap::first_cache (the process's first heap use), find_named, global_locale::setlocale, intern_name_locked, global_locale::replace.

## Hypothesis
Three costs, in order of size: (1) ~26 more faults: the first heap use (refill/first_cache) plus archive pages; setlocale may not need the heap at all for an archive locale (names and per-category views fit fixed static storage); (2) eager derivation at load: CtypeFile::parse + ascii_case_runs builds derived ctype data for every load, which belongs on the first use of the case functions; (3) Archive::lookup / codeset_matches / name copies. Measure each separately (perf on an undiluted "S" probe) before changing anything; drop any part that is under 5% of the row.

## Tests and plants
Existing setlocale / newlocale / ctype conformance and locale unit tests; add a test that toupper/tolower/isalpha give the archive's answers when first used after setlocale (spec section 7 and Appendix D.1). Plants: skip the deferred derivation on first use; return the C table after an en_US load; leave a stale name after a second setlocale.

## Rules
No eager work at start-up or on the hot path: lazy work stays on the first-use path, and that path is still measured (failure-path-cost rule). A table that is a pure function of build-time data is built at build time as read-only data; a table derived from host data (locale archive, TZif, .mo) is derived per entry on demand or cheaper. Re-time on the N100 timing host with cargo xtask bench --stage / --staged; Liella profiles by symbol on a build host; glibc aggregate counters and timing only.

**JOBS:** gate (direct tier, --base origin/next), plants (cargo xtask plants), counts (the probe deltas above, before/after/glibc), timing (N100 staged bench of the rows) as <host>:<label>.

First line of the hand-back: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:.
