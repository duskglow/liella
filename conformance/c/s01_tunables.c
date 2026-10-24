/* Spec §1.8 (D-1.8): GLIBC_TUNABLES switches the C layer's machine paths (D-4.14); the string
 * functions stay correct on every path. */
/* RUN-ENV: GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2,-ERMS:glibc.cpu.x86_rep_movsb_threshold=64:glibc.unknown.name=1 */
#define _POSIX_C_SOURCE 200809L
#include <stdlib.h>
#include <string.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

static char a[5000], b[5000];

int main(void)
{
    CHECK("s1_8_c_0", getenv("GLIBC_TUNABLES") != NULL);
    for (int n = 0; n < 4096; n += 37) {
        memset(a, 'x', (size_t)n);
        a[n] = 0;
        CHECK("s1_8_c_0", strlen(a) == (size_t)n && strnlen(a, 10) == (size_t)(n < 10 ? n : 10));
        memcpy(b, a, (size_t)n + 1);
        CHECK("s1_8_c_0", memcmp(a, b, (size_t)n + 1) == 0);
    }
    return 0;
}
