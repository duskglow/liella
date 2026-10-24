/* Row filter for the C benchmarks (BENCH-AFF). With CBENCH_ROWS=<row>,<row>,... in the environment, a
 * MEASURE row whose name is not in the list is skipped before its warm-up (`cb_skip` is the first statement
 * of every MEASURE macro): a targeted bench (`cargo xtask bench --rows ...` / `--affected`) times only the
 * rows a change can move plus the sentinel rows, not whole binaries. Unset or empty: every row runs and the
 * timed loops are unchanged (one cached test per row, outside the rounds). The setup code between rows
 * still runs, so the state a later row sees is the same except for what skipped rows themselves did (heap
 * shape, warm caches): the Liella and glibc builds skip the same rows, so the ratio stays like for like. */
#ifndef CBENCH_ROWS_H
#define CBENCH_ROWS_H
#include <stdlib.h>
#include <string.h>
/* The stress constructor (cbench_stress.h) runs in every process of a bench that includes it. A bench whose
 * rows measure its own child (cbench_startup re-executes /proc/self/exe) defines CBENCH_NO_STRESS first, so
 * the measured child stays exactly the base program: run 1 of STRESS-ENV-AUDIT-2 put the constructor's getenv
 * into startup_instr_c and moved it from about 2100 to 2197 instructions. */
#ifndef CBENCH_NO_STRESS
#include "cbench_stress.h"
#endif

static int cb_rows_read;
static const char *cb_rows;

static int cb_skip(const char *name)
{
    if (!cb_rows_read) {
        cb_rows_read = 1;
        cb_rows = getenv("CBENCH_ROWS");
        if (cb_rows && !cb_rows[0])
            cb_rows = NULL;
    }
    if (!cb_rows)
        return 0;
    size_t n = strlen(name);
    for (const char *p = cb_rows;;) {
        const char *e = strchr(p, ',');
        size_t l = e ? (size_t)(e - p) : strlen(p);
        if (l == n && memcmp(p, name, n) == 0)
            return 0;
        if (!e)
            return 1;
        p = e + 1;
    }
}
#endif
