Id: COLDSTART-FORK-CHILD-RESET
Kind: code
Priority: 55
Size: M
Host: a build host for counts and profiles; the N100 timing host (booked) for time
Model: opus
Effort: high
BUDGET: 60 tool calls / 120k
Stack on: next (730f416b or later)

# fork child reset cost

## Rows
cbench_process/fork_exit_wait: time 1.294x, instructions 1.081x (N100); 1.247x time, spread 0.67 (Ivy Bridge).

## Evidence
Probe, per fork+_exit+waitpid ((10 forks - 1 fork) / 9): Liella +6.8k user instructions, +295k kernel instructions, +27 page faults; glibc +0.3k user, +255k kernel, +12 faults (Broadwell-EP). The Liella profile of the fork loop (instructions:u): boundary::fork::child_reset 58% of the samples in the probe process.
Code: crates/boundary/src/fork.rs child_reset calls the after-fork hooks of thread, stacks, timer, mqueue, syslog, gai_async, aio, stdio, spawn and nscache unconditionally. Static data: the Liella probe has 136k .data + 512k .bss vs 3k + 142k for the dynamic glibc one.

## Hypothesis
Each hook writes its subsystem's state even when the process never used that subsystem, and each write to a page shared copy-on-write costs a fault (the +15 faults per fork) plus the instructions. Skip a hook whose subsystem was never initialised (a flag the subsystem sets on first use, kept together on one line read once), and make the hooks store only values that differ. Also check the parent side: a write after fork to a still-shared page costs a fault in the parent.

## Tests and plants
Existing fork / pthread_atfork / posix_spawn / timer / aio / stdio-after-fork tests (spec section 18). Add one: after fork in a process that used timers, aio and stdio, the child's state is reset (one per used subsystem). Plants: skip a hook for a used subsystem (one plant per hook); set the used flag never.

## Rules
No eager work at start-up or on the hot path: lazy work stays on the first-use path, and that path is still measured (failure-path-cost rule). Re-time on the N100 timing host with cargo xtask bench --stage / --staged; Liella profiles by symbol on a build host; glibc aggregate counters and timing only.

**JOBS:** gate (direct tier, --base origin/next), plants (cargo xtask plants), counts (the probe deltas above, before/after/glibc), timing (N100 staged bench of the rows) as <host>:<label>.

First line of the hand-back: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:.
