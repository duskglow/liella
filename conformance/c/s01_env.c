/* Spec §1.3: the environment's operations, with duplicate names from execve. */
/* RUN-ENV: DUP=1 DUP=2 KEEP=k */
#define _GNU_SOURCE
#include <errno.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

extern char **environ;

static int count(void)
{
    int n = 0;
    for (char **e = environ; e && *e; e++)
        n++;
    return n;
}

int main(void)
{
    static char put[] = "PUT=one";
    char val[] = "abc";
    CHECK("s1_3_f_3", strcmp(getenv("DUP"), "1") == 0);
    CHECK("s1_3_c_1", getenv("NOPE") == NULL && getenv("DU") == NULL && getenv("DUP=") == NULL);
    CHECK("s1_3_c_3", setenv("NEW", "v1", 0) == 0 && strcmp(getenv("NEW"), "v1") == 0);
    const char *old = getenv("NEW");
    CHECK("s1_3_c_3", setenv("NEW", "v2", 0) == 0 && strcmp(getenv("NEW"), "v1") == 0);
    CHECK("s1_3_c_3", setenv("NEW", "v2", 1) == 0 && strcmp(getenv("NEW"), "v2") == 0);
    CHECK("s1_3_g_1", strcmp(old, "v1") == 0);
    CHECK("s1_3_g_2", setenv("COPY", val, 1) == 0);
    val[0] = 'X';
    CHECK("s1_3_g_2", strcmp(getenv("COPY"), "abc") == 0);
    errno = 0;
    CHECK("s1_3_i_1", setenv("", "x", 1) == -1 && errno == EINVAL);
    errno = 0;
    CHECK("s1_3_i_1", setenv("A=B", "x", 1) == -1 && errno == EINVAL);
    errno = 0;
    CHECK("s1_3_i_1", unsetenv("") == -1 && errno == EINVAL);
    CHECK("s1_3_c_4", unsetenv("DUP") == 0 && getenv("DUP") == NULL);
    CHECK("s1_3_c_4", unsetenv("NEVER_SET") == 0);
    CHECK("s1_3_c_5", putenv(put) == 0 && strcmp(getenv("PUT"), "one") == 0);
    memcpy(put + 4, "two", 3);
    CHECK("s1_3_c_5", strcmp(getenv("PUT"), "two") == 0);
    CHECK("s1_3_c_5", putenv((char *)"PUT") == 0 && getenv("PUT") == NULL);
    /* Growth: the array is reallocated many times; every entry survives. */
    for (int i = 0; i < 300; i++) {
        char name[8] = "V000";
        name[1] = (char)('0' + i / 100);
        name[2] = (char)('0' + i / 10 % 10);
        name[3] = (char)('0' + i % 10);
        CHECK("s1_3_c_3", setenv(name, name, 1) == 0);
    }
    CHECK("s1_3_c_3", strcmp(getenv("V000"), "V000") == 0 && strcmp(getenv("V299"), "V299") == 0);
    CHECK("s1_3_c_2", secure_getenv("KEEP") != NULL && strcmp(secure_getenv("KEEP"), "k") == 0);
    CHECK("s1_3_c_6", clearenv() == 0 && getenv("KEEP") == NULL && count() == 0);
    CHECK("s1_3_c_6", setenv("AFTER", "1", 1) == 0 && strcmp(getenv("AFTER"), "1") == 0 && count() == 1);
    return 0;
}
