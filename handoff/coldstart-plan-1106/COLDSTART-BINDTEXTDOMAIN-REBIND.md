Id: COLDSTART-BINDTEXTDOMAIN-REBIND
Kind: code
Priority: 55
Size: S
Host: a build host for counts and profiles; the N100 timing host (booked) for time
Model: opus
Effort: high
BUDGET: 45 tool calls / 100k
Stack on: next (730f416b or later)

# Second bindtextdomain + dgettext with the same directory

## Rows
cbench_opt/second_bindtextdomain_gettext: time 7.7x (N100), 7.0x (Ivy Bridge). PERF-GETTEXT-LANGUAGE-STEADY-5 and PERF-GETTEXT-STEADY-2 are steady-state stories; nothing found that covers a rebind (not verified beyond their titles).

## Evidence
Probe, second "B" (after a full first pass): Liella +968 user instructions, +25k kernel instructions, +2 page faults; glibc +694 user, no kernel or fault increase (Broadwell-EP). The first "B" is cheaper on Liella (+34.7k user vs +135k). Liella page-fault profile of the run with the second pass: Heap::refill and machine_memset at the top, intl::translate present.

## Hypothesis
A bindtextdomain that names the directory the domain already has still drops or re-validates the loaded catalog, so the next dgettext reopens / re-reads it (the kernel instructions) and allocates fresh state (the faults: heap refill + zeroing). Keeping the loaded catalog when the new directory string equals the bound one keeps the observable behaviour (the result string and the translation are the same). Check the spec text for bindtextdomain (section 7.7) for anything a rebind must reset before changing it; if the spec does not decide whether a rebind re-reads a .mo changed on disk, raise a spec gap and black-box probe the reference (rewrite the .mo between two binds).

## Tests and plants
Existing gettext conformance and catalog unit tests; add: rebind to the same directory returns the same translation with no new open (count opens through the test seam, or strace -c in the test runner), and rebind to a different directory switches catalogs. Plants: never reload on a different directory; compare directories by pointer instead of content.

## Rules
No eager work at start-up or on the hot path: lazy work stays on the first-use path, and that path is still measured (failure-path-cost rule). Re-time on the N100 timing host with cargo xtask bench --stage / --staged; Liella profiles by symbol on a build host; glibc aggregate counters and timing only.

**JOBS:** gate (direct tier, --base origin/next), plants (cargo xtask plants), counts (the probe deltas above, before/after/glibc), timing (N100 staged bench of the rows) as <host>:<label>.

First line of the hand-back: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:.
