/* Spec §2.7: backtrace, backtrace_symbols and backtrace_symbols_fd in a program Liella started
 * (statically linked, not position-independent: the load bias is 0). Self-checking: each clause
 * ends with `end <clause>` on standard error, each failed check prints `FAIL <clause>: line N:
 * <check>`; the clause tests in crates/core/tests/s02_diagnostics_clauses.rs read these lines.
 *
 * The program's own functions sit in sections the linker lays out in name order
 * (`.text.sorted.*`), so each one's code ends where the next one starts and a return address can
 * be named by the function containing it. */
#define _GNU_SOURCE
#include <execinfo.h>
#include <fcntl.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

static int failures;
static const char *current;

static void begin(const char *clause) { current = clause; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

#define PLACED(n) __attribute__((noinline, noclone, section(".text.sorted.liella_bt_" #n)))

extern char __executable_start[], etext[];

static void *trace[64];
static int count;
static int limit = 64;

PLACED(01) int leaf(void) { count = backtrace(trace, limit); return count + 1; }
PLACED(02) int f2(void) { return leaf() + 1; }
PLACED(03) int f1(void) { return f2() + 1; }
PLACED(04) int driver(void) { return f1() + 1; }
PLACED(05) int sig_leaf(void) { count = backtrace(trace, limit); return count + 1; }
PLACED(06) void on_signal(int sig) { (void)sig; sig_leaf(); }
PLACED(07) int g3(void) { raise(SIGUSR1); return count + 1; }
PLACED(08) int g2(void) { return g3() + 1; }
PLACED(09) int g1(void) { return g2() + 1; }
PLACED(10) int sig_driver(void) { return g1() + 1; }
PLACED(11) int placed_end(void) { return 0; }

static void *const order[] = { (void *)leaf, (void *)f2, (void *)f1, (void *)driver, (void *)sig_leaf,
    (void *)on_signal, (void *)g3, (void *)g2, (void *)g1, (void *)sig_driver, (void *)placed_end };
static const char *const names[] = { "leaf", "f2", "f1", "driver", "sig_leaf", "on_signal", "g3", "g2", "g1",
    "sig_driver" };

/* The program function containing `a` (`main` included), or "~". */
static const char *name_of(void *a)
{
    uintptr_t x = (uintptr_t)a;
    for (unsigned i = 0; i + 1 < sizeof order / sizeof order[0]; i++)
        if (x > (uintptr_t)order[i] && x <= (uintptr_t)order[i + 1])
            return names[i];
    return "~";
}

/* The trace as names, separated by spaces (no stdio: it runs where no memory can be mapped). */
static void names_of(void **t, int n, char *dst, size_t cap)
{
    size_t k = 0;
    dst[0] = 0;
    for (int i = 0; i < n; i++) {
        const char *s = name_of(t[i]);
        size_t len = strlen(s);
        if (k + len + 2 >= cap)
            break;
        if (k)
            dst[k++] = ' ';
        memcpy(dst + k, s, len + 1);
        k += len;
    }
}

static int has(const char *text, const char *part)
{
    if (strstr(text, part))
        return 1;
    fprintf(stderr, "  [%s] lacks [%s]\n", text, part);
    return 0;
}

static int in_program(void *a) { return (char *)a >= __executable_start && (char *)a < etext; }

static int reap(pid_t pid)
{
    int st = -1;
    if (syscall(SYS_wait4, pid, &st, 0, NULL) != pid)
        return -1;
    return WIFEXITED(st) ? WEXITSTATUS(st) : 128 + WTERMSIG(st);
}

/* Touch 1 MiB of stack, so that the limit set below (which also stops the stack growing) leaves
 * the calls room. */
__attribute__((noinline)) static void grow_stack(void)
{
    volatile char room[1 << 20];
    for (size_t i = 0; i < sizeof room; i += 4096)
        room[i] = 1;
}

/* Bits a child reports in its exit status. */
enum { FIRST_IN_HANDLER = 1, THROUGH_TRAMPOLINE = 2, FD_COMPLETE = 4, FD_NO_MEMORY = 8 };

/* In a fresh child, before this process has called any backtrace function or allocated memory:
 * no more memory can be mapped, and the first backtrace call is made from a signal handler; then
 * backtrace_symbols_fd writes it. Returns the FIRST_* bits; the written text goes to `fd`. */
static int first_call_child(int fd)
{
    grow_stack();
    char buf[64];
    int sf = open("/proc/self/statm", O_RDONLY);
    ssize_t n = sf >= 0 ? read(sf, buf, sizeof buf - 1) : -1;
    if (n <= 0)
        return 0;
    buf[n] = 0;
    long pages = strtol(buf, NULL, 10);
    close(sf);
    struct rlimit rl = { (rlim_t)pages * 4096, RLIM_INFINITY };
    if (setrlimit(RLIMIT_AS, &rl) != 0)
        return 0;
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_signal;
    sigaction(SIGUSR1, &sa, NULL);
    sig_driver();
    int bits = 0;
    char seq[512];
    names_of(trace, count, seq, sizeof seq);
    if (strncmp(seq, "sig_leaf on_signal ", 19) == 0)
        bits |= FIRST_IN_HANDLER;
    if (strstr(seq, " g3 g2 g1 sig_driver"))
        bits |= THROUGH_TRAMPOLINE;
    if ((bits & (FIRST_IN_HANDLER | THROUGH_TRAMPOLINE)) != (FIRST_IN_HANDLER | THROUGH_TRAMPOLINE)) {
        if (write(2, "  first trace: ", 15) < 0 || write(2, seq, strlen(seq)) < 0 || write(2, "\n", 1) < 0)
            bits = 0;
    }
    backtrace_symbols_fd(trace, count, fd);
    if (count > 0)
        bits |= FD_COMPLETE;
    void *more = malloc(1 << 24);
    if (more == NULL)
        bits |= FD_NO_MEMORY; /* the limit was real: nothing could have been allocated */
    return bits;
}

int main(int argc, char **argv)
{
    (void)argc;
    int pfd[2];
    if (pipe(pfd) != 0)
        return 2;
    pid_t first = fork();
    if (first == 0)
        _exit(first_call_child(pfd[1]));
    close(pfd[1]);
    int first_bits = reap(first);
    static char fd_text[8192];
    ssize_t fn = read(pfd[0], fd_text, sizeof fd_text - 1);
    fd_text[fn > 0 ? fn : 0] = 0;
    close(pfd[0]);

    int layout_ok = 1;
    for (unsigned i = 0; i + 1 < sizeof order / sizeof order[0]; i++)
        layout_ok &= (uintptr_t)order[i] < (uintptr_t)order[i + 1];
    if (!layout_ok) {
        fprintf(stderr, "the functions are not laid out in order: the checks cannot run\n");
        return 1;
    }
    char seq[1024];

    begin("s2_7_c_1");
    limit = 64;
    driver();
    int full = count;
    void *saved[64];
    memcpy(saved, trace, sizeof saved);
    names_of(trace, count, seq, sizeof seq);
    /* The current call stack, innermost first: the library's own frames are left out (D-2.7). */
    CHECK(has(seq, "leaf f2 f1 driver ") && strncmp(seq, "leaf ", 5) == 0);
    for (int n = 0; n <= 5; n++) {
        limit = n;
        memset(trace, 0, sizeof trace);
        driver();
        /* The innermost frames are the same; main's return address differs (another call). */
        size_t same = (size_t)(n < 4 ? n : 4);
        CHECK(count == n && memcmp(trace, saved, same * sizeof(void *)) == 0 && trace[n] == NULL);
    }
    limit = 64;
    CHECK(backtrace(trace, -1) == 0);
    end();

    begin("s2_7_c_2");
    {
        char **s = backtrace_symbols(saved, full);
        CHECK(s != NULL);
        if (s) {
            /* One block: the pointer array, then the texts it points to; freeing it frees all. */
            size_t total = (size_t)full * sizeof(char *);
            for (int i = 0; i < full; i++)
                total += strlen(s[i]) + 1;
            for (int i = 0; i < full; i++)
                CHECK(s[i] >= (char *)(s + full) && s[i] + strlen(s[i]) < (char *)s + total + 64);
            for (int i = 0; i < full; i++) {
                char want[256];
                snprintf(want, sizeof want, "%s(+%p) [%p]", argv[0], saved[i], saved[i]);
                if (strcmp(s[i], want) != 0) {
                    fprintf(stderr, "  got  [%s]\n  want [%s]\n", s[i], want);
                    CHECK(!"one description per address");
                }
            }
            free(s);
        }
        CHECK(backtrace_symbols(saved, 0) != NULL);
    }
    end();

    begin("s2_7_c_3");
    {
        FILE *f = tmpfile();
        backtrace_symbols_fd(saved, full, fileno(f));
        char got[8192], want[8192];
        ssize_t n = pread(fileno(f), got, sizeof got - 1, 0);
        got[n > 0 ? n : 0] = 0;
        size_t k = 0;
        want[0] = 0;
        for (int i = 0; i < full; i++)
            k += (size_t)snprintf(want + k, sizeof want - k, "%s(+%p)[%p]\n", argv[0], saved[i], saved[i]);
        if (strcmp(got, want) != 0)
            fprintf(stderr, "  got  [%s]\n  want [%s]\n", got, want);
        CHECK(strcmp(got, want) == 0);
        /* Without allocating: a child that could not map any memory wrote a whole trace. */
        CHECK((first_bits & (FD_COMPLETE | FD_NO_MEMORY)) == (FD_COMPLETE | FD_NO_MEMORY));
        int lines = 0;
        for (const char *p = fd_text; (p = strstr(p, "]\n")) != NULL; p += 2)
            lines++;
        CHECK(lines >= 8 && fd_text[strlen(fd_text) - 1] == '\n'); /* sig_leaf ... sig_driver, main */
        backtrace_symbols_fd(saved, 0, fileno(f));
        backtrace_symbols_fd(saved, -1, fileno(f));
        CHECK(lseek(fileno(f), 0, SEEK_END) == n);
        fclose(f);
    }
    end();

    begin("s2_7_f_1");
    /* D-2.7: Liella's own unwinder, so the first call needs no loading and is async-signal-safe:
     * made first from a signal handler, with no memory to map, it traces through the signal
     * frame to the interrupted functions. */
    CHECK(first_bits >= 0 && (first_bits & FIRST_IN_HANDLER) != 0);
    CHECK((first_bits & THROUGH_TRAMPOLINE) != 0);
    /* The same from a handler on an alternate signal stack (SA_ONSTACK): the handler's frames
     * and the interrupted ones are on different stacks, in either address order, and the trace
     * still crosses from one to the other. */
    {
        static char alt[1 << 16];
        stack_t ss = { .ss_sp = alt, .ss_size = sizeof alt, .ss_flags = 0 };
        struct sigaction sa, old;
        memset(&sa, 0, sizeof sa);
        sa.sa_handler = on_signal;
        sa.sa_flags = SA_ONSTACK;
        CHECK(sigaltstack(&ss, NULL) == 0 && sigaction(SIGUSR1, &sa, &old) == 0);
        limit = 64;
        count = 0;
        sig_driver();
        /* Only the handler writes `count`: without a barrier the compiler may keep the 0. */
        __asm__ volatile("" ::: "memory");
        names_of(trace, count, seq, sizeof seq);
        int alt_ok = strncmp(seq, "sig_leaf on_signal ", 19) == 0 && has(seq, " g3 g2 g1 sig_driver");
        CHECK(alt_ok);
        if (!alt_ok) {
            fprintf(stderr, "  alternate-stack trace (%d): %s\n ", count, seq);
            for (int i = 0; i < count; i++)
                fprintf(stderr, " %p", trace[i]);
            fprintf(stderr, " [order %p..%p]\n", order[0], order[sizeof order / sizeof order[0] - 1]);
        }
        ss.ss_flags = SS_DISABLE;
        CHECK(sigaltstack(&ss, NULL) == 0 && sigaction(SIGUSR1, &old, NULL) == 0);
    }
    end();

    begin("s2_7_f_2");
    /* Every frame reported is a real one: each address lies in the program's code, the frames
     * with call-frame information are all present and in order, and the trace ends at the
     * outermost frame. */
    for (int i = 0; i < full; i++)
        CHECK(in_program(saved[i]));
    CHECK(full < 64);
    names_of(saved, full, seq, sizeof seq);
    CHECK(has(seq, "leaf f2 f1 driver"));
    end();

    begin("s2_7_f_3");
    {
        /* D-2.7: no symbol names (a static program has no dynamic symbols): an address in the
         * program is `program(+offset) [address]`; any other address is `[address]`. */
        char want[64];
        void *outside[2] = { (void *)0x10, (void *)want };
        char **s = backtrace_symbols(outside, 2);
        CHECK(s != NULL && strcmp(s[0], "[0x10]") == 0);
        snprintf(want, sizeof want, "[%p]", outside[1]);
        CHECK(s != NULL && strcmp(s[1], want) == 0);
        free(s);
    }
    end();

    return failures ? 1 : 0;
}
