/* Spec §2.3: the BSD (err, warn) and GNU (error, error_at_line) diagnostic helpers through real
 * C varargs. Self-checking: each clause ends with `end <clause>` on standard error, each failed
 * check prints `FAIL <clause>: line N: <check>`; the clause tests in
 * crates/core/tests/s02_diagnostics_clauses.rs read these lines.
 *
 * Each case runs in a child process whose standard output and error both go to one file (so
 * their order shows), with the program names set as a shell would: `program_invocation_name`
 * `/usr/bin/prog`, `program_invocation_short_name` `prog` (D-2.3: the BSD family names the
 * short one, the GNU family the full one). */
#define _GNU_SOURCE
#include <err.h>
#include <errno.h>
#include <error.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

static int failures;
static const char *current;

static void begin(const char *clause) { current = clause; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

static int eq(const char *got, const char *want)
{
    if (strcmp(got, want) == 0)
        return 1;
    fprintf(stderr, "  got  [%s]\n  want [%s]\n", got, want);
    return 0;
}

struct run {
    int status; /* the exit status; 77 when the case returned; -1 for a signal */
    char out[4096];
};

/* Run `body` in a child whose standard output and error go to one file. */
static void run(struct run *r, void (*body)(void))
{
    fflush(NULL);
    FILE *f = tmpfile();
    pid_t pid = fork();
    if (pid == 0) {
        alarm(10);
        dup2(fileno(f), 1);
        dup2(fileno(f), 2);
        body();
        fflush(NULL);
        _exit(77);
    }
    int st = 0;
    syscall(SYS_wait4, pid, &st, 0, NULL);
    r->status = WIFEXITED(st) ? WEXITSTATUS(st) : -1;
    ssize_t n = pread(fileno(f), r->out, sizeof r->out - 1, 0);
    r->out[n > 0 ? n : 0] = 0;
    fclose(f);
}

static void my_vwarn(const char *fmt, ...) { va_list ap; va_start(ap, fmt); vwarn(fmt, ap); va_end(ap); }
static void my_vwarnx(const char *fmt, ...) { va_list ap; va_start(ap, fmt); vwarnx(fmt, ap); va_end(ap); }
static void my_verr(int st, const char *fmt, ...) { va_list ap; va_start(ap, fmt); verr(st, fmt, ap); }
static void my_verrx(int st, const char *fmt, ...) { va_list ap; va_start(ap, fmt); verrx(st, fmt, ap); }

static void at_exit(void) { fputs("[atexit]\n", stderr); }

/* ------------------------------------------------------------------ the cases */

static void c0_warn(void)
{
    errno = ENOENT;
    warn("file %s", "x.txt");
    errno = EACCES;
    warnx("plain %d", 42);
    errno = EBADF;
    my_vwarn("v %d", 1);
    my_vwarnx("vx %d", 2);
    errno = 0;
    error(0, EPERM, "gnu %s", "form");
    error(0, 0, "no number");
}

static void c1_err(void) { atexit(at_exit); errno = EPERM; err(3, "cannot %s", "go"); }
static void c1_errx(void) { errx(5, "bye %c", '!'); }
static void c1_verr(void) { errno = EIO; my_verr(7, "v%d", 1); }
static void c1_verrx(void) { my_verrx(8, "vx%d", 2); }

static void c2_count(void)
{
    unsigned before = error_message_count;
    error(0, 0, "one");
    error(0, ENOENT, "two");
    warnx("not counted");
    printf("[count %u]\n", error_message_count - before);
}

static void c2_flush(void)
{
    /* Pending on stdout (a file: fully buffered); error flushes it before writing. */
    printf("[pending]");
    error(0, 0, "after");
    printf("[later]");
    fflush(stdout);
}

static void c2_exit(void) { atexit(at_exit); printf("[pending]"); error(6, EINTR, "stop"); fputs("[returned]\n", stderr); }

static void c3_at_line(void)
{
    char other[] = "b.c"; /* the same name at another address */
    error_at_line(0, 0, "a.c", 10, "first");
    error_at_line(0, EBADF, "a.c", 10, "again");
    error_one_per_line = 1;
    error_at_line(0, 0, "b.c", 7, "once");
    error_at_line(0, 0, "b.c", 7, "repeated");
    error_at_line(0, 0, other, 7, "repeated, another pointer");
    error_at_line(0, 0, "b.c", 8, "next line");
    error_at_line(0, 0, "a.c", 8, "other file");
    error_at_line(0, 0, "b.c", 8, "back to b.c");
    printf("[count %u]\n", error_message_count);
}

static void hook(void) { fputs("HOOK|", stderr); }

static void c4_hook(void)
{
    error_print_progname = hook;
    error(0, 0, "with hook");
    error_at_line(0, ENOENT, "h.c", 5, "at line");
    error_print_progname = NULL;
    error(0, 0, "without");
}

int main(void)
{
    static struct run r;
    program_invocation_name = "/usr/bin/prog";
    program_invocation_short_name = "prog";

    begin("s2_3_c_0");
    run(&r, c0_warn);
    CHECK(r.status == 77);
    CHECK(eq(r.out, "prog: file x.txt: No such file or directory\n"
                    "prog: plain 42\n"
                    "prog: v 1: Bad file descriptor\n"
                    "prog: vx 2\n"
                    "/usr/bin/prog: gnu form: Operation not permitted\n"
                    "/usr/bin/prog: no number\n"));
    end();

    begin("s2_3_c_1");
    run(&r, c1_err);
    CHECK(r.status == 3 && eq(r.out, "prog: cannot go: Operation not permitted\n[atexit]\n"));
    run(&r, c1_errx);
    CHECK(r.status == 5 && eq(r.out, "prog: bye !\n"));
    run(&r, c1_verr);
    CHECK(r.status == 7 && eq(r.out, "prog: v1: Input/output error\n"));
    run(&r, c1_verrx);
    CHECK(r.status == 8 && eq(r.out, "prog: vx2\n"));
    end();

    begin("s2_3_c_2");
    run(&r, c2_count);
    CHECK(r.status == 77 && eq(r.out, "/usr/bin/prog: one\n/usr/bin/prog: two: No such file or directory\nprog: not counted\n[count 2]\n"));
    run(&r, c2_flush);
    CHECK(r.status == 77 && eq(r.out, "[pending]/usr/bin/prog: after\n[later]"));
    run(&r, c2_exit);
    CHECK(r.status == 6 && eq(r.out, "[pending]/usr/bin/prog: stop: Interrupted system call\n[atexit]\n"));
    end();

    begin("s2_3_c_3");
    run(&r, c3_at_line);
    CHECK(r.status == 77 && eq(r.out, "/usr/bin/prog:a.c:10: first\n"
                                      "/usr/bin/prog:a.c:10: again: Bad file descriptor\n"
                                      "/usr/bin/prog:b.c:7: once\n"
                                      "/usr/bin/prog:b.c:8: next line\n"
                                      "/usr/bin/prog:a.c:8: other file\n"
                                      "/usr/bin/prog:b.c:8: back to b.c\n"
                                      "[count 6]\n"));
    end();

    begin("s2_3_c_4");
    run(&r, c4_hook);
    CHECK(r.status == 77 && eq(r.out, "HOOK|with hook\nHOOK|h.c:5: at line: No such file or directory\n/usr/bin/prog: without\n"));
    end();

    return failures ? 1 : 0;
}
