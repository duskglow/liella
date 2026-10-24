/* The environment contract (D-1.10, ENVIRON-TABLE-CONTRACT), end to end.
 *
 * Part 1 ("#### off") runs against both C libraries and its output must be identical: with the
 * contract off (the default) getenv sees every change at once, made through setenv, putenv,
 * unsetenv and clearenv or directly (a slot stored, environ replaced, a putenv string's name
 * edited in place), and across fork and exec.
 *
 * Part 2 ("#### contract") runs only where liella_environ_contract exists (Liella; the reference
 * has no such name, so the weak reference is NULL): with the contract on, the writers keep
 * lookups current; a change made directly is not seen until liella_environ_reload; a replaced
 * environ is seen at once; fork keeps the table; exec starts with the contract off. Each check
 * prints "ok <what>" or "FAIL <what>". */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;
#ifdef LIELLA_PRODUCTS
/* Strong references: a weak one would not pull the definitions out of the static libc.a. */
int liella_environ_contract(int on);
void liella_environ_reload(void);
#else
int liella_environ_contract(int on) __attribute__((weak));
void liella_environ_reload(void) __attribute__((weak));
#endif

static const char *show(const char *name)
{
    const char *v = getenv(name);
    return v ? v : "(null)";
}

static void print(const char *what, const char *a, const char *b)
{
    printf("%s: %s=%s", what, a, show(a));
    if (b)
        printf(" %s=%s", b, show(b));
    printf("\n");
}

static int slot_of(const char *name)
{
    size_t len = strlen(name);
    for (int i = 0; environ && environ[i]; i++)
        if (strncmp(environ[i], name, len) == 0 && environ[i][len] == '=')
            return i;
    return -1;
}

static int fails;
#define EXPECT(what, cond)                                                                       \
    do {                                                                                         \
        if (cond)                                                                                \
            printf("ok %s\n", what);                                                             \
        else {                                                                                   \
            printf("FAIL %s (line %d)\n", what, __LINE__);                                       \
            fails++;                                                                             \
        }                                                                                        \
    } while (0)
#define IS(name, want) ((want) ? (getenv(name) && strcmp(getenv(name), (want)) == 0) : getenv(name) == NULL)


/* Run this program again with `arg`; its output follows ours. */
static void exec_child(const char *arg)
{
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        execl("/proc/self/exe", "env_contract", arg, (char *)NULL);
        _exit(127);
    }
    int status = 0;
    waitpid(pid, &status, 0);
    printf("exec child exit %d\n", WIFEXITED(status) ? WEXITSTATUS(status) : -1);
}

static void fill(int n)
{
    static char filler[120];
    memset(filler, 'x', sizeof filler - 1);
    for (int k = 0; k < n; k++) {
        char name[24];
        snprintf(name, sizeof name, "AAA_STRESS_%04d", k);
        setenv(name, filler, 1);
    }
}

static void off_part(void)
{
    printf("#### off\n");
    clearenv();
    fill(300);
    setenv("A", "1", 1);
    setenv("B", "2", 1);
    print("setenv", "A", "B");
    printf("miss: NOPE=%s\n", show("NOPE"));
    setenv("A", "3", 1);
    setenv("B", "x", 0);
    print("replace", "A", "B");
    static char p[] = "P=1";
    putenv(p);
    print("putenv", "P", NULL);
    p[2] = '2';
    print("putenv value edited", "P", NULL);
    p[0] = 'Q';
    print("putenv name edited", "P", "Q");
    static char direct_a[] = "A=direct", direct_z[] = "Z=slot";
    environ[slot_of("A")] = direct_a;
    print("slot stored", "A", NULL);
    environ[slot_of("B")] = direct_z;
    print("slot renamed", "B", "Z");
    unsetenv("Z");
    print("unsetenv", "Z", "A");
    static char r1[] = "R=1";
    static char *mine[4] = { r1, NULL };
    environ = mine;
    print("environ replaced", "R", "A");
    clearenv();
    print("clearenv", "R", NULL);
    setenv("S", "1", 1);
    print("after clearenv", "S", NULL);
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        setenv("T", "child", 1);
        print("fork child", "S", "T");
        fflush(stdout);
        _exit(0);
    }
    waitpid(pid, NULL, 0);
    print("fork parent", "S", "T");
    exec_child("off-child");
}

static void contract_part(void)
{
    printf("#### contract\n");
    clearenv();
    EXPECT("off before the first call", liella_environ_contract(1) == 0);
    EXPECT("on after it", liella_environ_contract(1) == 1);
    EXPECT("empty environment: a miss", IS("A", NULL));
    fill(300);
    setenv("A", "1", 1);
    setenv("B", "2", 1);
    EXPECT("setenv adds", IS("A", "1") && IS("B", "2") && IS("NOPE", NULL));
    EXPECT("300 variables", IS("AAA_STRESS_0000", NULL) == 0 && IS("AAA_STRESS_0299", NULL) == 0 && IS("AAA_STRESS_0300", NULL));
    setenv("A", "3", 1);
    setenv("B", "x", 0);
    EXPECT("setenv replaces, or not without overwrite", IS("A", "3") && IS("B", "2"));
    static char p[] = "P=1", p2[] = "P=new";
    putenv(p);
    EXPECT("putenv adds", IS("P", "1"));
    p[2] = '2';
    EXPECT("an edited putenv value is read at once", IS("P", "2"));
    putenv(p2);
    EXPECT("putenv replaces", IS("P", "new"));
    p2[0] = 'Q';
    EXPECT("an edited putenv name is not seen", IS("Q", NULL));
    liella_environ_reload();
    EXPECT("until reload", IS("Q", "new") && IS("P", NULL));
    static char direct_z[] = "Z=slot", direct_b[] = "B=direct";
    environ[slot_of("B")] = direct_b;
    EXPECT("a slot stored with the same name is read at once", IS("B", "direct"));
    environ[slot_of("B")] = direct_z;
    EXPECT("a slot renamed: the old name is gone, the new one not seen", IS("B", NULL) && IS("Z", NULL));
    liella_environ_reload();
    EXPECT("until reload", IS("Z", "slot") && IS("B", NULL));
    unsetenv("Z");
    unsetenv("NOT_SET");
    EXPECT("unsetenv removes", IS("Z", NULL) && IS("A", "3") && IS("Q", "new"));
    putenv("A");
    EXPECT("putenv without '=' removes", IS("A", NULL) && IS("Q", "new"));
    fill(2000);
    setenv("LAST", "l", 1);
    int all = 1;
    for (int k = 0; k < 2000; k += 97) {
        char name[24];
        snprintf(name, sizeof name, "AAA_STRESS_%04d", k);
        all &= getenv(name) != NULL;
    }
    EXPECT("2,000 variables (the table grows)", all && IS("LAST", "l") && IS("AAA_STRESS_2000", NULL) && IS("Q", "new"));
    static char r1[] = "R=1";
    static char *mine[4] = { r1, NULL };
    char **saved = environ;
    environ = mine;
    EXPECT("a replaced environ is seen at once", IS("R", "1") && IS("LAST", NULL));
    environ = saved;
    EXPECT("and the table's own again", IS("R", NULL) && IS("LAST", "l"));
    clearenv();
    EXPECT("clearenv empties", IS("LAST", NULL) && IS("Q", NULL));
    setenv("S", "1", 1);
    EXPECT("setenv after clearenv", IS("S", "1"));
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        setenv("T", "child", 1);
        EXPECT("fork child: the table is inherited and kept current", IS("S", "1") && IS("T", "child"));
        EXPECT("fork child: still on", liella_environ_contract(1) == 1);
        fflush(stdout);
        _exit(fails != 0);
    }
    int status = 0;
    waitpid(pid, &status, 0);
    EXPECT("fork child passed", WIFEXITED(status) && WEXITSTATUS(status) == 0);
    EXPECT("fork parent: the child's setenv is not here", IS("T", NULL) && IS("S", "1"));
    exec_child("contract-child");
    EXPECT("off again: the previous state was on", liella_environ_contract(0) == 1);
    environ[slot_of("S")] = direct_z;
    EXPECT("off: a stored slot is seen at once", IS("Z", "slot") && IS("S", NULL));
    liella_environ_reload();
    EXPECT("reload while off does nothing", IS("Z", "slot"));
    static char direct_o[] = "O2=x";
    setenv("OFF", "1", 1);
    environ[slot_of("OFF")] = direct_o;
    EXPECT("off: after a writer call, a stored slot is still seen at once", IS("O2", "x") && IS("OFF", NULL));
}

int main(int argc, char **argv)
{
    setvbuf(stdout, NULL, _IOLBF, 0);
    if (argc > 1 && strcmp(argv[1], "off-child") == 0) {
        print("exec child", "S", "T");
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "contract-child") == 0) {
        EXPECT("exec child: the contract is off in a new image", liella_environ_contract(0) == 0);
        EXPECT("exec child: the environment arrived", IS("S", "1"));
        return fails != 0;
    }
    off_part();
    if (liella_environ_contract)
        contract_part();
    printf("fails %d\n", fails);
    return fails != 0;
}
