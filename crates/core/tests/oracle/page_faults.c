/* Page faults of a short program (PAGEFAULTS-2): argv[1] = "true" (startup only), "strings" (see main), "locale"
 * (setlocale(LC_ALL, "") + malloc + printf, the shape of most command-line tools) or "tool" (the
 * shape of a coreutils program such as printf: HOTTEXT-FAULT-TEST) or "fork" (a fork child's faults,
 * see main). Prints the
 * process's own minor-fault count at the end (getrusage RUSAGE_SELF ru_minflt) on stderr. */
#include <fcntl.h>
#include <inttypes.h>
#include <libintl.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static size_t (*volatile len_fn)(const char *) = strlen;
static int (*volatile ncmp_fn)(const char *, const char *, size_t) = strncmp;
static void *(*volatile mchr_fn)(const void *, int, size_t) = memchr;
static int (*volatile mcmp_fn)(const void *, const void *, size_t) = memcmp;
static char *(*volatile scpy_fn)(char *, const char *) = strcpy;

static void at_exit_hook(void) {}

static long minflt(void) {
    struct rusage ru;
    return getrusage(RUSAGE_SELF, &ru) == 0 ? ru.ru_minflt : -1000000;
}

static void put32(unsigned char *p, uint32_t v) {
    for (int i = 0; i < 4; i++)
        p[i] = (unsigned char)(v >> (8 * i));
}

/* A two-entry catalog (header, "hello" -> "hallo") in the .mo layout the GNU gettext manual
 * documents: magic, revision, count, the two string-table offsets, an empty hash table. */
static int write_mo(const char *path) {
    static const char header[] = "Content-Type: text/plain; charset=UTF-8\n";
    const char *orig[2] = {"", "hello"}, *trans[2] = {header, "hallo"};
    unsigned char buf[256] = {0};
    uint32_t n = 2, o = 28, t = o + 8 * n, at = t + 8 * n;
    put32(buf, 0x950412de);
    put32(buf + 8, n);
    put32(buf + 12, o);
    put32(buf + 16, t);
    put32(buf + 24, at);
    for (uint32_t i = 0; i < 2 * n; i++) {
        const char *s = i < n ? orig[i] : trans[i - n];
        uint32_t len = (uint32_t)strlen(s), e = i < n ? o + 8 * i : t + 8 * (i - n);
        put32(buf + e, len);
        put32(buf + e + 4, at);
        memcpy(buf + at, s, len);
        at += len + 1;
    }
    int f = open(path, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
    if (f < 0)
        return -1;
    int ok = write(f, buf, at) == (ssize_t)at;
    close(f);
    return ok ? 0 : -1;
}

/* vfork, then execve(path, argv, envp) in the child (exit 127 if it fails), as raw system calls in
 * one block: the child runs no code and touches no memory before exec, and the program links no
 * library function beyond what its modes already use (vfork, execve, waitpid and getauxval from
 * libc.a moved the program's library layout: setlocale + printf and tool +2). Returns the child's
 * pid or -errno. */
static long relay_spawn(const char *path, char **argv, char **envp) {
    long r;
    __asm__ volatile("mov $58, %%eax\n\t" /* SYS_vfork */
                     "syscall\n\t"
                     "test %%rax, %%rax\n\t"
                     "jnz 1f\n\t"
                     "mov $59, %%eax\n\t" /* SYS_execve; rdi, rsi, rdx survive syscall */
                     "syscall\n\t"
                     "mov $127, %%edi\n\t"
                     "mov $60, %%eax\n\t" /* SYS_exit */
                     "syscall\n"
                     "1:"
                     : "=a"(r)
                     : "D"(path), "S"(argv), "d"(envp)
                     : "rcx", "r11", "memory");
    return r;
}

int main(int argc, char **argv, char **envp) {
    if (argc > 2 && strcmp(argv[1], "relay") == 0) {
        /* STARTUP-FAULTS-RESTORE-4: "relay <mode>" runs <mode> in a vfork child of this program. The
         * whole-process counts ("true", "locale", "tool") include what the counted process does
         * between fork/spawn and exec, which is the spawning process's code: the test harness spawns
         * through the host C library in one gate pass and through Liella's interposed exports in the
         * other, and the two read one apart. Relayed, the counted process does nothing before exec,
         * whoever spawned this program. The child execs the path this program was exec'd by
         * (AT_EXECFN, 31, from the auxiliary vector after envp) with the same environment, so its
         * stack strings are those of an unrelayed run. */
        char *cargv[] = {argv[0], argv[2], NULL};
        char **e = envp;
        while (*e != NULL)
            e++;
        const char *self = NULL;
        for (unsigned long *a = (unsigned long *)(e + 1); a[0] != 0; a += 2)
            if (a[0] == 31)
                self = (const char *)a[1];
        if (self == NULL)
            return 7;
        long p = relay_spawn(self, cargv, envp);
        struct rusage ru;
        int st;
        if (p < 0 || wait4((pid_t)p, &st, 0, &ru) != p)
            return 5;
        return WIFEXITED(st) ? WEXITSTATUS(st) : 6;
    }
    if (argc > 1 && strcmp(argv[1], "fork") == 0) {
        /* TIMING-COV-FORK-COW-FAULTS: "fork" = fork + _exit(0) + wait4, twice; prints the minor
         * faults of the second child (wait4's rusage: from fork to exit). fork copies no page
         * tables of the file-backed text, so the child faults once per 64 KiB text window its
         * path touches, plus its copy-on-write writes. */
        long child = -1;
        for (int i = 0; i < 2; i++) {
            pid_t p = fork();
            if (p == 0)
                _exit(0);
            struct rusage ru;
            int st;
            if (p < 0 || wait4(p, &st, 0, &ru) != p || !WIFEXITED(st) || WEXITSTATUS(st) != 0)
                return 4;
            child = ru.ru_minflt;
        }
        fprintf(stderr, "minflt %ld\n", child);
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "rebind") == 0) {
        /* COLDSTART-BINDTEXTDOMAIN-REBIND-2: a catalog looked up, then bound again to the same
         * directory and looked up again (cbench_opt's second pass); prints only the faults of the
         * second bindtextdomain + dgettext (the repeat lookup's text is in the hot group). */
        setlocale(LC_ALL, "");
        char dir[] = "/tmp/liella-pf-rebind.XXXXXX", sub[96], mo[128];
        if (mkdtemp(dir) == NULL)
            return 1;
        snprintf(sub, sizeof sub, "%s/en_US", dir);
        mkdir(sub, 0700);
        snprintf(sub, sizeof sub, "%s/en_US/LC_MESSAGES", dir);
        mkdir(sub, 0700);
        snprintf(mo, sizeof mo, "%s/pfrebind.mo", sub);
        long n = write_mo(mo) == 0;
        n += bindtextdomain("pfrebind", dir) != NULL && strcmp(dgettext("pfrebind", "hello"), "hallo") == 0;
        n += strcmp(dgettext("pfrebind-absent", "no catalog"), "no catalog") == 0;
        long f0 = minflt();
        n += bindtextdomain("pfrebind", dir) != NULL && strcmp(dgettext("pfrebind", "hello"), "hallo") == 0;
        long f1 = minflt();
        unlink(mo);
        rmdir(sub);
        snprintf(sub, sizeof sub, "%s/en_US", dir);
        rmdir(sub);
        rmdir(dir);
        fprintf(stderr, "minflt %ld\n", n == 4 ? f1 - f0 : -1);
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "mktime") == 0) {
        /* PERF-FIRSTCALL-MKTIME-SNPRINTF: the first mktime after localtime_r loaded the zone
         * (cbench_opt's cold rows); prints only that call's faults. The zone's one-time setup
         * belongs to the load, not to this call. */
        setenv("TZ", "America/New_York", 1);
        time_t when = 1759579200;
        struct tm r, t;
        long n = localtime_r(&when, &r) != NULL;
        t = r;
        t.tm_isdst = -1;
        long f0 = minflt();
        n += mktime(&t) == when;
        long f1 = minflt();
        fprintf(stderr, "minflt %ld\n", n == 2 ? f1 - f0 : -1);
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "localtime") == 0) {
        /* PERF-ZONE-LOAD-FAULTS: the first localtime_r, which reads and parses the zone file
         * (cbench_opt's first_localtime_r row); prints only that call's faults. */
        setenv("TZ", "America/New_York", 1);
        time_t when = 1759579200;
        struct tm r;
        long f0 = minflt();
        long n = localtime_r(&when, &r) != NULL;
        long f1 = minflt();
        fprintf(stderr, "minflt %ld\n", n == 1 && r.tm_isdst == 1 ? f1 - f0 : -1);
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "localtime_mktime") == 0) {
        /* PERF-ZONE-LOAD-FAULTS-2: the first localtime_r (zone load included) and the first
         * mktime after it, counted together, so a fault that only moves from one call to the
         * other shows as no change and one that is added shows in the sum. */
        setenv("TZ", "America/New_York", 1);
        time_t when = 1759579200;
        struct tm r, t;
        long f0 = minflt();
        long n = localtime_r(&when, &r) != NULL;
        t = r;
        t.tm_isdst = -1;
        n += mktime(&t) == when;
        long f1 = minflt();
        fprintf(stderr, "minflt %ld\n", n == 2 && r.tm_isdst == 1 ? f1 - f0 : -1);
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "float") == 0) {
        /* PERF-FIRSTCALL-MKTIME-SNPRINTF: the faults of a program's first float conversion
         * (cbench_opt's first_snprintf shape); the common %.2f must not probe a large stack frame. */
        char b[64];
        volatile double v = 3.25;
        long f0 = minflt();
        long n = snprintf(b, sizeof b, "%s=%d (%.2f)", "key", 42, v) == 13;
        long f1 = minflt();
        fprintf(stderr, "minflt %ld\n", n == 1 ? f1 - f0 : -1);
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "strings") == 0) {
        /* RC1-FIRST: "strings" = after printf, the first calls of the common string functions;
         * prints only the faults those calls took (their entry points are in the hot group). */
        printf("%s %d\n", "a", 5);
        fflush(stdout);
        char b[64];
        const char *volatile s = "a line of text: compared";
        long f0 = minflt();
        long n = (long)len_fn(s) + ncmp_fn(s, "a line", 6) + (mchr_fn(s, ':', 20) != NULL) + mcmp_fn(s, "a l", 3);
        n += scpy_fn(b, s) == b;
        long f1 = minflt();
        fprintf(stderr, "minflt %ld\n", n == 26 ? f1 - f0 : -1);
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "locale") == 0) {
        setlocale(LC_ALL, "");
        char *volatile p = malloc(100);
        printf("%s %d\n", p ? "a" : "b", 5);
        fflush(stdout);
        free(p);
    }
    if (argc > 1 && strcmp(argv[1], "tool") == 0) {
        /* HOTTEXT-FAULT-TEST: what a coreutils tool (printf "%s %d %x\n" a 1 1) runs before and
         * after main: setlocale, bindtextdomain/textdomain, atexit, getenv, strtoimax, printf,
         * then a stdout flush. A stale hot-text list (functions of this path outside the hot
         * group) shows as extra file-backed text faults; PRINTF-LOOP-FAULTS: 14 -> 17 pages. */
        setlocale(LC_ALL, "");
        bindtextdomain("coreutils", "/nonexistent/locale");
        textdomain("coreutils");
        atexit(at_exit_hook);
        const char *volatile e = getenv("POSIXLY_CORRECT");
        const char *volatile num = "17";
        char *end;
        intmax_t v = strtoimax(num, &end, 10);
        unsigned long x = strtoumax("1f", &end, 16);
        printf("%s %jd %lx %d\n", e ? "p" : "a", v, x, (int)(end != NULL));
        if (fflush(stdout) != 0)
            return 3;
    }
    struct rusage ru;
    if (getrusage(RUSAGE_SELF, &ru) != 0)
        return 2;
    fprintf(stderr, "minflt %ld\n", ru.ru_minflt);
    return 0;
}
