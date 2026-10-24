/* Milestone-6 probe (gawk-printf profile): cost of snprintf("%.3f") against the size the
 * caller passes, under glibc and Liella. */
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec / 1e9; }
int main(void)
{
    static char buf[1 << 20];
    size_t sizes[] = { 64, 4096, 65536, 1 << 20 };
    const char *fmts[] = { "%.3f", "%d", "%s", "%x" };
    for (int f = 0; f < 4; f++)
        for (int s = 0; s < 4; s++) {
            double t0 = now();
            for (int i = 0; i < 200000; i++) {
                if (f == 0) snprintf(buf, sizes[s], fmts[f], i / 7.0);
                else if (f == 2) snprintf(buf, sizes[s], fmts[f], "ab");
                else snprintf(buf, sizes[s], fmts[f], i);
            }
            printf("%-5s size %7zu: %.1f ns/call\n", fmts[f], sizes[s], (now() - t0) / 200000 * 1e9);
        }
    return 0;
}
