/* Spec §1.1, §1.4, §1.5, §2.1: the state Liella's runtime sets up before main.
 * Each failing CHECK exits with its line number. */
/* RUN-ARGS: alpha beta */
/* RUN-ENV: CONF=1 HOME=/home/x */
#define _GNU_SOURCE
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

extern char **environ;
extern char **__environ;
extern char **_environ;
extern char *__progname;
extern char *__progname_full;
extern void *__libc_stack_end;

static __thread int tdata = 41;
static __thread char tbss[256];
static __thread long tls_aligned __attribute__((aligned(64))) = 7;

static int ctor_order[4];
static int ctor_n;
__attribute__((constructor(101))) static void first(void) { ctor_order[ctor_n++] = 1; }
__attribute__((constructor(102))) static void second(void) { ctor_order[ctor_n++] = 2; }

static unsigned long canary(void)
{
    unsigned long v;
    __asm__("movq %%fs:0x28, %0" : "=r"(v));
    return v;
}

/* An array local: -fstack-protector-strong checks the canary on return. */
static int protected_copy(const char *s)
{
    char buf[64];
    strcpy(buf, s);
    return (int)strlen(buf);
}

int main(int argc, char **argv, char **envp)
{
    int n = 0;
    CHECK("s1_1_c_1", argc == 3 && strcmp(argv[1], "alpha") == 0 && strcmp(argv[2], "beta") == 0);
    CHECK("s1_1_g_1", argv[argc] == NULL);
    CHECK("s1_1_c_2", environ == envp && __environ == envp && _environ == envp);
    CHECK("s1_2_i_2", EXIT_SUCCESS == 0 && EXIT_FAILURE == 1);
    while (envp[n])
        n++;
    CHECK("s1_1_g_2", n == 2);
    CHECK("s1_3_c_1", strcmp(getenv("CONF"), "1") == 0 && strcmp(getenv("HOME"), "/home/x") == 0);
    CHECK("s1_4_c_0", program_invocation_name == argv[0] && __progname_full == argv[0]);
    const char *slash = strrchr(argv[0], '/');
    CHECK("s1_4_c_0", strcmp(program_invocation_short_name, slash ? slash + 1 : argv[0]) == 0);
    CHECK("s1_4_c_0", __progname == program_invocation_short_name);
    CHECK("s1_1_c_3", tdata == 41 && tbss[100] == 0 && tls_aligned == 7);
    CHECK("s1_1_c_3", ((unsigned long)&tls_aligned % 64) == 0);
    tdata++;
    tbss[255] = 1;
    CHECK("s1_1_c_3", tdata == 42 && tbss[255] == 1);
    unsigned long c = canary();
    CHECK("s1_1_c_3", c != 0 && (c & 0xff) == 0);
    CHECK("s1_1_c_3", protected_copy("hello") == 5);
    CHECK("s1_1_c_5", ctor_n == 2 && ctor_order[0] == 1 && ctor_order[1] == 2);
    CHECK("s1_5_c_0", getauxval(AT_PAGESZ) == 4096);
    CHECK("s1_5_g_0", getauxval(AT_RANDOM) != 0 && getauxval(AT_PHDR) != 0);
    errno = 0;
    CHECK("s1_5_f_0", getauxval(12345) == 0 && errno == ENOENT);
    errno = 77;
    CHECK("s2_1_c_0", errno == 77 && __errno_location() == &errno);
    CHECK("s1_1_c_0", __libc_stack_end != NULL && (char *)&n < (char *)__libc_stack_end);
    return 0;
}
