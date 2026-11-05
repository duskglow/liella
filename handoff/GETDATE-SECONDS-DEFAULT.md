# GETDATE-SECONDS-DEFAULT

State: branch task/getdate-seconds-default. Result: Liella getdate was already right; the probe was time-dependent.

Cause (no product change). getdate zeroes unset smaller fields once a time is given (D-23.7 family, crates/legacy/src/getdate.rs,
unit test posix_rules). The probe (time_gaps_probe.rs) sets TZ so the clock reads 12:30:00 at the test's instant, then prints a
result field equal to the current one as "cur". The clock keeps running, so the local second is the run's delay mod 60. In a
call that landed in local second 0, a correct "sec=0" printed as "sec=cur" on Liella's side only (glibc's run was in another
second), so the lines differed. Earlier trains passed because the Liella run's calls rarely land in second 0 of a minute
(only when the delay past the instant is a multiple of 60 s; CI train 380 was slow enough). Not a landing change:
getdate.rs and the probe only changed in the public-tree and midnight-carry commits.

Fix: gd() in the probe waits (100 ms steps, at most 2 s, no wall assertion) until the current local second is not 0.
Regression: unit case "%a %H:%M" "Wed 10:00" -> seconds 0 (current 39).
Plants (3, all CAUGHT by posix_rules): seconds/minutes default to current, hour-no-move.
Open: the same masking exists for min/hour when the delay reaches 15+ min (not realistic); not changed.
Jobs on keke: getdate-seconds-default-gate (rc 0), getdate-seconds-default-plants (rc 0). Clippy and static clean.
Not verified: the probe fix was not forced into second 0 (the wait is by construction).
