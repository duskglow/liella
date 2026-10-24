/* The expected-crash helper for C test programs (CORE-STORM; the Rust twin is
 * liella_boundary::selftest::expect_crash). Call expect_crash() in the process that is about to
 * crash ON PURPOSE (abort, fortify failure, failed assertion, a probe that may fault), immediately
 * before the crash. It makes the process non-dumpable, so the kernel neither writes a core file
 * nor starts the core-pipe helper (apport) for it: with a pipe core_pattern the helper starts for
 * every crash even under RLIMIT_CORE 0, and a storm of them made a test VM unreachable.
 *
 * Never call it in a process that goes on to run other checks: an UNEXPECTED crash must still
 * dump. `cargo xtask crash-audit` checks every expected-crash site reaches it (escape:
 * a `dump-ok: <why>` comment in the file).
 *
 * Not a conformance program itself (run.sh compiles only *.c). Uses prctl from <sys/prctl.h>,
 * which every program links against the library under test. */
#ifndef LIELLA_EXPECT_CRASH_H
#define LIELLA_EXPECT_CRASH_H
#include <sys/prctl.h>

static inline void expect_crash(void)
{
    prctl(PR_SET_DUMPABLE, 0, 0, 0, 0);
}
#endif
