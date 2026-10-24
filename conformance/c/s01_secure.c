/* Spec §1.1 contract clause 7 and guarantee 4, §1.3 secure_getenv, §1.8: in secure-execution
 * mode (here a setgid program) the unsafe variables are removed before main, no tunable is
 * honoured, and secure_getenv returns NULL. */
/* RUN-SECURE: yes */
/* RUN-ENV: LD_PRELOAD=/nonexistent.so SAFE=1 MALLOC_ARENA_MAX=1 GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 TMPDIR=/tmp LD_LIBRARY_PATH=/x LOCPATH=/y HOSTALIASES=/h RES_OPTIONS=ndots:7 LOCALDOMAIN=evil.test TZDIR=/z GCONV_PATH=/g NLSPATH=/n PWD=/ MALLOC_CHECK_=3 */
#define _GNU_SOURCE
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

extern char **environ;

int main(void)
{
    int n = 0;
    CHECK("s1_1_c_7", getauxval(AT_SECURE) == 1);
    CHECK("s1_1_g_4", getenv("LD_PRELOAD") == NULL && getenv("LD_LIBRARY_PATH") == NULL);
    CHECK("s1_1_g_4", getenv("MALLOC_ARENA_MAX") == NULL && getenv("GLIBC_TUNABLES") == NULL);
    CHECK("s1_1_g_4", getenv("TMPDIR") == NULL && getenv("LOCPATH") == NULL);
    CHECK("s1_1_g_4", getenv("SAFE") != NULL && strcmp(getenv("SAFE"), "1") == 0);
    /* Liella's own security-sensitive variables (owner ruling 2026-10-03): all removed. */
    CHECK("s1_1_g_4", getenv("HOSTALIASES") == NULL && getenv("RES_OPTIONS") == NULL && getenv("LOCALDOMAIN") == NULL);
    CHECK("s1_1_g_4", getenv("TZDIR") == NULL && getenv("GCONV_PATH") == NULL && getenv("NLSPATH") == NULL);
    /* Kept: PWD (ignored where it is read) and names outside the list. */
    CHECK("s1_1_g_4", getenv("PWD") != NULL && getenv("MALLOC_CHECK_") != NULL);
    while (environ[n])
        n++;
    CHECK("s1_1_g_4", n == 3); /* SAFE, PWD, MALLOC_CHECK_ */
    CHECK("s1_3_c_2", secure_getenv("SAFE") == NULL);
    return 0;
}
