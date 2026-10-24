/* §5.1, D-5.5, D-7.8: the per-thread table pointers `__ctype_b_loc`, `__ctype_toupper_loc` and
 * `__ctype_tolower_loc` expose are always current, and the byte functions read them.
 *
 * Behaviour settled by black-box probes against glibc 2.39 (PJ-22): a pointer fetched once
 * follows `setlocale` in the same thread and `uselocale` (an object, LC_GLOBAL_LOCALE), and the
 * tables stay after `freelocale` of the active object; a new
 * thread starts with the global locale's tables whatever its creator uses; a forked child keeps
 * its parent's. One deliberate difference (docs/differences-from-glibc.md, D-7.8): another
 * thread's `setlocale` reaches every thread using the global locale at once (glibc: only the
 * calling thread's pointers change).
 *
 * The checks need a single-byte locale whose classes differ from "C" above 0x7f: de_DE in
 * ISO-8859-1 (0xe9 is a lowercase letter, 0xc9 its uppercase), compiled with the host's
 * `localedef` into a private directory named by LOCPATH, removed at exit. */
#define _GNU_SOURCE
#include <ctype.h>
#include <langinfo.h>
#include <locale.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wchar.h>
#include <wctype.h>

#define DE "de_DE.ISO-8859-1"

static int failures;
static char dir[256];

static void check(int ok, const char *what) {
    if (!ok) {
        printf("FAIL: %s\n", what);
        fflush(stdout);
        __atomic_add_fetch(&failures, 1, __ATOMIC_SEQ_CST);
    }
}

/* Whether the thread classifies by de: through the given slot addresses, through fresh calls
 * and through the functions; all must agree with `de`. */
static void expect(const char *what, const unsigned short **pb, const int **pu, const int **pl, int de) {
    char m[160];
    int up = de ? 0xc9 : 0xe9, low = de ? 0xe9 : 0xc9;
    snprintf(m, sizeof m, "%s: slot class", what);
    check((((*pb)[0xe9] & _ISalpha) != 0) == de, m);
    snprintf(m, sizeof m, "%s: slot toupper", what);
    check((*pu)[0xe9] == up, m);
    snprintf(m, sizeof m, "%s: slot tolower", what);
    check((*pl)[0xc9] == low, m);
    snprintf(m, sizeof m, "%s: same slot addresses", what);
    check(__ctype_b_loc() == pb && __ctype_toupper_loc() == pu && __ctype_tolower_loc() == pl, m);
    snprintf(m, sizeof m, "%s: isalpha/islower/isupper", what);
    check(((isalpha)(0xe9) != 0) == de && ((islower)(0xe9) != 0) == de && ((isupper)(0xc9) != 0) == de, m);
    snprintf(m, sizeof m, "%s: toupper/tolower", what);
    check((toupper)(0xe9) == up && (tolower)(0xc9) == low, m);
    snprintf(m, sizeof m, "%s: isctype", what);
    check((isctype(0xe9, _ISalpha) != 0) == de, m);
    snprintf(m, sizeof m, "%s: out of range unchanged", what);
    check((toupper)(1000) == 1000 && (tolower)(-129) == -129 && !(isalpha)(256) && (toupper)(-1) == -1, m);
    /* D-7.13: the functions that read the thread's locale view follow the same changes. */
    snprintf(m, sizeof m, "%s: iswalpha/towupper/towlower/wcwidth (view)", what);
    check(((iswalpha)(0xe9) != 0) == de && (towupper)(0xe9) == (wint_t)up && (towlower)(0xc9) == (wint_t)low && wcwidth(0xe9) == (de ? 1 : -1), m);
    snprintf(m, sizeof m, "%s: CODESET (view)", what);
    check(strcmp(nl_langinfo(CODESET), de ? "ISO-8859-1" : "ANSI_X3.4-1968") == 0, m);
}

static void here(const char *what, int de) {
    expect(what, __ctype_b_loc(), __ctype_toupper_loc(), __ctype_tolower_loc(), de);
}

static void cleanup(void) {
    if (dir[0]) {
        char cmd[300];
        snprintf(cmd, sizeof cmd, "rm -rf '%s'", dir);
        if (system(cmd) != 0)
            printf("could not remove %s\n", dir);
    }
}

static void prepare(void) {
    const char *tmp = getenv("TMPDIR");
    snprintf(dir, sizeof dir, "%s/s05-ctype-words-XXXXXX", tmp && *tmp ? tmp : "/tmp");
    if (!mkdtemp(dir)) {
        dir[0] = 0;
        printf("FAIL: mkdtemp\n");
        exit(1);
    }
    atexit(cleanup);
    char cmd[400];
    snprintf(cmd, sizeof cmd, "localedef -i de_DE -f ISO-8859-1 '%s/" DE "' >/dev/null 2>&1; cd '%s' && ln -s " DE " de_DE.iso88591", dir, dir);
    if (system(cmd) == -1 || setenv("LOCPATH", dir, 1) != 0 || !setlocale(LC_ALL, DE)) {
        printf("FAIL: compiling and loading " DE " (localedef from the locales package)\n");
        exit(1);
    }
    setlocale(LC_ALL, "C");
}

static void one_thread(void) {
    const unsigned short **pb = __ctype_b_loc();
    const int **pu = __ctype_toupper_loc(), **pl = __ctype_tolower_loc();
    const unsigned short *initial = *pb;
    expect("initial", pb, pu, pl, 0);
    check((*pb)[-1] == 0 && (*pu)[-1] == -1 && (*pl)[-1] == -1, "EOF entries");
    setlocale(LC_ALL, DE);
    expect("setlocale de, fetched before", pb, pu, pl, 1);
    locale_t c = newlocale(LC_ALL_MASK, "C", (locale_t)0);
    uselocale(c);
    expect("uselocale(C object)", pb, pu, pl, 0);
    check(*pb == initial, "the C object's table is the initial one");
    setlocale(LC_ALL, "C");
    setlocale(LC_ALL, DE);
    expect("own C object, global changed", pb, pu, pl, 0);
    uselocale(LC_GLOBAL_LOCALE);
    expect("uselocale(LC_GLOBAL_LOCALE)", pb, pu, pl, 1);
    setlocale(LC_ALL, "C");
    expect("global back to C", pb, pu, pl, 0);
    locale_t d = newlocale(LC_ALL_MASK, DE, (locale_t)0);
    uselocale(d);
    expect("uselocale(de object)", pb, pu, pl, 1);
    locale_t d2 = newlocale(LC_ALL_MASK, "C", (locale_t)0);
    uselocale(d2);
    expect("object to object (C)", pb, pu, pl, 0);
    freelocale(d);
    locale_t e = newlocale(LC_ALL_MASK, DE, (locale_t)0);
    uselocale(e);
    freelocale(d2);
    expect("object to object (de)", pb, pu, pl, 1);
    uselocale(LC_GLOBAL_LOCALE);
    freelocale(c);
    expect("global C again", pb, pu, pl, 0);
    /* The active object given to newlocale as its base (D-7.13; in a child, since the thread's
     * locale is then the consumed base): Liella reuses the block (D-7.3) and the view follows
     * the changed object; glibc returns a new object (probe 2a4d6d50f8da), the active one is
     * then undefined and nothing is checked. */
    pid_t q = fork();
    if (q == 0) {
        locale_t g = newlocale(LC_ALL_MASK, DE, (locale_t)0);
        uselocale(g);
        int before = iswalpha(0xe9) && towupper(0xe9) == 0xc9;
        locale_t g2 = newlocale(LC_CTYPE_MASK, "C", g);
        if (g2 != g)
            _exit(!before);
        _exit(!(before && !iswalpha(0xe9) && towupper(0xe9) == 0xe9 && strcmp(nl_langinfo(CODESET), "ANSI_X3.4-1968") == 0));
    }
    int qs = 0;
    waitpid(q, &qs, 0);
    check(WIFEXITED(qs) && WEXITSTATUS(qs) == 0, "active object changed in place: wide functions and CODESET follow");
    /* The active object freed: the tables stay (read through the slots only; in a child, since
     * the thread's locale is then undefined). */
    pid_t p = fork();
    if (p == 0) {
        uselocale(e);
        freelocale(e);
        _exit(!(((*pb)[0xe9] & _ISalpha) && (*pu)[0xe9] == 0xc9));
    }
    int st = 0;
    waitpid(p, &st, 0);
    check(WIFEXITED(st) && WEXITSTATUS(st) == 0, "after freelocale of the active object");
    freelocale(e);
}

static pthread_barrier_t bar;
static locale_t de;

/* One phase: the main thread changes the global locale, then (A) the threads check, then (B)
 * the main thread goes on. */
static void wait_a(void) { pthread_barrier_wait(&bar); }
static void wait_b(void) { pthread_barrier_wait(&bar); }

static void *follower(void *a) {
    (void)a;
    const unsigned short **pb = __ctype_b_loc();
    const int **pu = __ctype_toupper_loc(), **pl = __ctype_tolower_loc();
    wait_a();
    expect("thread on the global locale, main set de", pb, pu, pl, 1);
    wait_b();
    wait_a();
    expect("thread on the global locale, main set C", pb, pu, pl, 0);
    wait_b();
    wait_a();
    wait_b();
    return 0;
}

static void *own(void *a) {
    (void)a;
    const unsigned short **pb = __ctype_b_loc();
    const int **pu = __ctype_toupper_loc(), **pl = __ctype_tolower_loc();
    uselocale(de);
    wait_a();
    expect("thread with its own de, main set de", pb, pu, pl, 1);
    wait_b();
    wait_a();
    expect("thread with its own de, main set C", pb, pu, pl, 1);
    uselocale(LC_GLOBAL_LOCALE);
    expect("that thread back on the global locale", pb, pu, pl, 0);
    wait_b();
    wait_a();
    expect("that thread, main set de again", pb, pu, pl, 1);
    wait_b();
    return 0;
}

static void phase(const char *name) {
    setlocale(LC_ALL, name);
    wait_a();
    wait_b();
}

static void *fresh(void *a) {
    here((const char *)a, 1);
    return 0;
}

static void threads(void) {
    de = newlocale(LC_ALL_MASK, DE, (locale_t)0);
    check(de != 0, "newlocale de");
    pthread_barrier_init(&bar, 0, 3);
    pthread_t t1, t2, t3;
    pthread_create(&t1, 0, follower, 0);
    pthread_create(&t2, 0, own, 0);
    phase(DE);
    phase("C");
    phase(DE);
    pthread_join(t1, 0);
    pthread_join(t2, 0);
    pthread_create(&t3, 0, fresh, "new thread under global de");
    pthread_join(t3, 0);
    locale_t c = newlocale(LC_ALL_MASK, "C", (locale_t)0);
    uselocale(c);
    pthread_create(&t3, 0, fresh, "new thread, creator on its own C");
    pthread_join(t3, 0);
    /* Many threads following the global locale while it changes. */
    uselocale(LC_GLOBAL_LOCALE);
    setlocale(LC_ALL, "C");
    pthread_t many[8];
    pthread_barrier_destroy(&bar);
    pthread_barrier_init(&bar, 0, 9);
    for (int i = 0; i < 8; i++)
        pthread_create(&many[i], 0, follower, 0);
    phase(DE);
    phase("C");
    phase("C");
    for (int i = 0; i < 8; i++)
        pthread_join(many[i], 0);
    uselocale(c);
    setlocale(LC_ALL, DE);
}

static void forks(void) {
    /* this thread: its own C over the global de */
    const unsigned short **pb = __ctype_b_loc();
    const int **pu = __ctype_toupper_loc(), **pl = __ctype_tolower_loc();
    pid_t p = fork();
    if (p == 0) {
        expect("child of a thread on its own C", pb, pu, pl, 0);
        uselocale(LC_GLOBAL_LOCALE);
        expect("child, uselocale(LC_GLOBAL_LOCALE)", pb, pu, pl, 1);
        _exit(failures != 0);
    }
    int st = 0;
    waitpid(p, &st, 0);
    check(WIFEXITED(st) && WEXITSTATUS(st) == 0, "first child");
    uselocale(LC_GLOBAL_LOCALE);
    p = fork();
    if (p == 0) {
        expect("child of a thread on the global de", pb, pu, pl, 1);
        setlocale(LC_ALL, "C");
        expect("child, setlocale C", pb, pu, pl, 0);
        _exit(failures != 0);
    }
    waitpid(p, &st, 0);
    check(WIFEXITED(st) && WEXITSTATUS(st) == 0, "second child");
    expect("parent after its children", pb, pu, pl, 1);
}

int main(void) {
    setvbuf(stdout, 0, _IONBF, 0);
    prepare();
    one_thread();
    threads();
    forks();
    if (failures)
        printf("%d failures\n", failures);
    return failures != 0;
}
