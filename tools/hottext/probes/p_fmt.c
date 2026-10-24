/* hot-text probe (RC2-FAULT): the common printf conversions into a buffer and a stream, floats included
 * (the first snprintf of a program took 17 faults on RC2's cbench_opt: its float and buffer paths were
 * outside the hot group). */
#include <stdio.h>
#include <string.h>

int main(void)
{
    char buf[256];
    volatile double d = 3.25, g = 1e-5;
    volatile long l = -1234567;
    int n = snprintf(buf, sizeof buf, "%s=%d (%.2f) %ld %u %x %c %5s %g %e %%", "key", 42, d, l, 7u, 255u, 'z', "ab", g, d);
    n += printf("%s %d %.3f\n", buf, n, d);
    n += fprintf(stderr, "%-8s|%08.3f|%zu\n", "w", d, strlen(buf));
    return n > 0 ? 0 : 1;
}
