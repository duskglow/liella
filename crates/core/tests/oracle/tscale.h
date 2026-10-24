/* Load-tolerant watchdogs for the oracle drivers (task FL-9).
 *
 * `tw_alarm(secs)` is `alarm(secs)` with the limit multiplied by a scale that grows with the load of
 * the machine, so a child that is only slow (a shared gate machine, 16 test threads on few CPUs) is not
 * killed by a limit sized for an idle one, while a child that is really hung still dies, at most 10x
 * later. Keep the formula in step with crates/core/tests/support/timeout.rs and oracle/tscale.sh:
 *   pressure = max(load1, running tasks - 1); scale = clamp(1 + ceil(pressure / cpus), 2, 10)
 * LIELLA_TEST_TIMEOUT_SCALE=<n> overrides it (1..100). `alarm(0)` stays a cancel. */
#ifndef LIELLA_TSCALE_H
#define LIELLA_TSCALE_H
#include <fcntl.h>
#include <limits.h>
#include <stdlib.h>
#include <unistd.h>

static unsigned tw_scale(void)
{
    static unsigned cached;
    if (cached)
        return cached;
    const char *e = getenv("LIELLA_TEST_TIMEOUT_SCALE");
    if (e && *e) {
        long v = atol(e);
        return cached = v < 1 ? 1 : v > 100 ? 100 : (unsigned)v;
    }
    long pressure = 0; /* hundredths of a runnable task */
    char b[128];
    int fd = open("/proc/loadavg", O_RDONLY);
    ssize_t n = fd < 0 ? -1 : read(fd, b, sizeof b - 1);
    if (fd >= 0)
        close(fd);
    if (n > 0) {
        b[n] = 0;
        const char *p = b;
        long whole = 0, frac = 0, digits = 0;
        while (*p >= '0' && *p <= '9')
            whole = whole * 10 + (*p++ - '0');
        if (*p == '.')
            for (p++; *p >= '0' && *p <= '9'; p++, digits++)
                if (digits < 2)
                    frac = frac * 10 + (*p - '0');
        if (digits == 1)
            frac *= 10;
        pressure = whole * 100 + frac;
        for (int sp = 0; sp < 2 && *p; p++)
            sp += *p == ' ';
        for (p++; *p && *p != ' '; p++) /* skip the 5-minute and 15-minute averages, then "running/total" */
            ;
        while (*p == ' ')
            p++;
        long run = 0;
        while (*p >= '0' && *p <= '9')
            run = run * 10 + (*p++ - '0');
        if (*p == '/' && (run - 1) * 100 > pressure)
            pressure = (run - 1) * 100;
    }
    long cpus = sysconf(_SC_NPROCESSORS_ONLN);
    if (cpus < 1)
        cpus = 1;
    long s = 1 + (pressure + 100 * cpus - 1) / (100 * cpus);
    return cached = s < 2 ? 2 : s > 10 ? 10 : (unsigned)s;
}

static unsigned tw_alarm(unsigned secs)
{
    unsigned long long l = (unsigned long long)secs * tw_scale();
    return alarm(l > UINT_MAX ? UINT_MAX : (unsigned)l);
}
#endif
