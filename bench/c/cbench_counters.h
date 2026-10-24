/* Per-row hardware counters for the C benchmarks (MEASURE-1). With CBENCH_COUNTERS=1 in the
 * environment, each MEASURE row also counts, over all of its rounds, the process's aggregate
 * hardware events (perf_event_open on the process itself; no sampling, no symbols) and writes
 * `ctr<TAB>name<TAB>instructions<TAB>cycles<TAB>branch-misses<TAB>L1d-read-misses<TAB>user-instructions<TAB>uops`
 * per operation to standard error (-1: the event is not available). Unset: one test of a cached
 * flag per row, outside the timed loop; the timed loop is unchanged. The ioctls that start and
 * stop the counters run once per row, outside the rounds. `cargo xtask bench --compare
 * --counters` reads these lines; the timing of a counted run is not used. */
#ifndef CBENCH_COUNTERS_H
#define CBENCH_COUNTERS_H
#include <cpuid.h>
#include <linux/perf_event.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/syscall.h>
#include <unistd.h>

#ifndef SYS_perf_event_open
#define SYS_perf_event_open 298
#endif

/* uops: Intel's uops_issued.any (event 0x0E, umask 0x01; Haswell and later) as a raw event, only on a
 * GenuineIntel CPU (the same raw code means something else on AMD); -1 where the PMU lacks it (VMs). */
/* Events 0 (instructions), 1 (cycles) and 5 (uops) form ONE pinned group: they are always counted together over the
 * whole row. Without it, 6 events on a CPU with 3-4 programmable counters were multiplexed and the scaled values of
 * short rows were wrong (nico, Ivy Bridge: memcmp_16 showed 0.4 cycles/op). The other three events may multiplex. */
#define CTR_N 6
static int ctr_fd[CTR_N] = {-1, -1, -1, -1, -1, -1};
static int ctr_on = -1;

static void ctr_open(void)
{
    const char *v = getenv("CBENCH_COUNTERS");
    ctr_on = v && v[0] == '1';
    if (!ctr_on)
        return;
    static const struct {
        unsigned type;
        unsigned long long config;
        int user_only;
    } ev[CTR_N] = {
        {PERF_TYPE_HARDWARE, PERF_COUNT_HW_INSTRUCTIONS, 0},
        {PERF_TYPE_HARDWARE, PERF_COUNT_HW_CPU_CYCLES, 0},
        {PERF_TYPE_HARDWARE, PERF_COUNT_HW_BRANCH_MISSES, 0},
        {PERF_TYPE_HW_CACHE,
         PERF_COUNT_HW_CACHE_L1D | (PERF_COUNT_HW_CACHE_OP_READ << 8) | (PERF_COUNT_HW_CACHE_RESULT_MISS << 16), 0},
        {PERF_TYPE_HARDWARE, PERF_COUNT_HW_INSTRUCTIONS, 1},
        {PERF_TYPE_RAW, 0x010E, 0},
    };
    unsigned ea = 0, eb = 0, ec = 0, ed = 0;
    int intel = __get_cpuid(0, &ea, &eb, &ec, &ed) && eb == 0x756e6547u && ed == 0x49656e69u && ec == 0x6c65746eu;
    for (int i = 0; i < CTR_N; i++) {
        if (i == 5 && !intel)
            continue;
        struct perf_event_attr a;
        memset(&a, 0, sizeof a);
        a.size = sizeof a;
        a.type = ev[i].type;
        a.config = ev[i].config;
        a.disabled = 1;
        a.pinned = i == 0; /* instructions + cycles + uops: one pinned group, never multiplexed out */
        a.exclude_hv = 1;
        a.exclude_kernel = (unsigned)ev[i].user_only;
        a.read_format = PERF_FORMAT_TOTAL_TIME_ENABLED | PERF_FORMAT_TOTAL_TIME_RUNNING;
        ctr_fd[i] = (int)syscall(SYS_perf_event_open, &a, 0, -1, (i == 1 || i == 5) ? ctr_fd[0] : -1, 0UL);
    }
}

static void ctr_begin(void)
{
    if (ctr_on < 0)
        ctr_open();
    if (!ctr_on)
        return;
    for (int i = 0; i < CTR_N; i++) {
        if (ctr_fd[i] >= 0) {
            ioctl(ctr_fd[i], PERF_EVENT_IOC_RESET, 0);
            ioctl(ctr_fd[i], PERF_EVENT_IOC_ENABLE, 0);
        }
    }
}

static void ctr_end(const char *name, double ops)
{
    if (!ctr_on)
        return;
    double per[CTR_N];
    for (int i = 0; i < CTR_N; i++)
        if (ctr_fd[i] >= 0)
            ioctl(ctr_fd[i], PERF_EVENT_IOC_DISABLE, 0);
    for (int i = 0; i < CTR_N; i++) {
        unsigned long long r[3] = {0, 0, 0};
        per[i] = -1.0;
        if (ctr_fd[i] >= 0 && read(ctr_fd[i], r, sizeof r) == (ssize_t)sizeof r && r[2] > 0)
            per[i] = (double)r[0] * ((double)r[1] / (double)r[2]) / ops;
    }
    char buf[256];
    int n = snprintf(buf, sizeof buf, "ctr\t%s\t%.3f\t%.3f\t%.4f\t%.4f\t%.3f\t%.3f\n", name, per[0], per[1], per[2], per[3], per[4], per[5]);
    if (n > 0)
        (void)!write(2, buf, (size_t)n < sizeof buf ? (size_t)n : sizeof buf - 1);
}
#endif
