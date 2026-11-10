# TIMING-COV-COLD-STRCOLL: the first strcoll in en_US.UTF-8 costs about 20-24x glibc

Id: TIMING-COV-COLD-STRCOLL
Kind: code
Priority: 68
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 calls / 160k
Stacks-on: none

## The change
Cut the cold first `strcoll` in a rules locale (en_US.UTF-8). D-4.54 (plan/07-decisions/s04) REJECTED the lazy
"fill only its own bytes" design (measured slower, reverted in COLDSTART-STRCOLL-FIRST-2): do not retry it. D-4.54
records that the first call's time is NOT in the fill: the rejected design cut the first call's user instructions
from 11,749 to 1,601 and the row still got slower (3 to 4 page faults, about +13k kernel instructions). Escalation
#3369 (base): the first strcoll costs 3 page faults and ~30k kernel instructions, glibc 0 faults and ~0.5k.

## Rows (1106E, time IVB / N100)
cbench_opt/first_strcoll 23.930 / 20.146 (spread 0.027 / 0.213). second_strcoll and strcoll_en_us_* are not slower.

## Evidence
- Build host (Skylake-SP class, counters-mei.txt): first_strcoll 11.7 us Liella vs 0.28 us glibc.
- D-4.54 Zen 1 probe (quoted above): faults, not instructions, carry the first call.

## Hypothesis (to verify)
The faults come from first touches of collation data pages (archive mapping read on first use) and/or a first
heap/bss page the fill writes. Attribute each fault (perf record -e page-faults with addresses, Liella only;
or mincore/probe deltas around the call), then move the data so the first compare touches pages setlocale already
touched, or prefault them inside setlocale only if setlocale's own row does not get slower (check with
TIMING-COV-COLD-SETLOCALE's rows: the two stories touch the same path; coordinate via the coordinator, no stacking).

## Tests and plants
strcoll/strxfrm conformance and the locale tests on the direct tier; plants on each changed function.

## Rows to time
first_strcoll, second_strcoll, first_setlocale_en_US, cold_exec_once, strcoll_en_us_* (cbench_locale) and the sort
validation workload's strcoll share if the layout moves.
## JOBS
Gate: `cargo xtask clippy`, `cargo xtask static`, then the direct tier (`cargo xtask affected --run --base origin/next`)
through jobrun on your role. Plants: `cargo xtask plants <file>` on every changed function (msg: regex per plant).
Counts before/after on base and head (region counters or the probe, both sides printed). Timing: book an Ivy Bridge
and an N100 host (`ops timing-book`), stage once with `cargo xtask bench --stage`, run the rows below base vs head
vs glibc interleaved (>= 3 rounds), cancel the bookings when done. Keep only a change that makes no measured row slower.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
