/* Spec §6.2 (clauses s6_2_c_1, s6_2_c_1_1, s6_2_c_3), D-6.10 (WL-HOT-SEQ3/SEQ4): the single-character
 * quick path of mbrtowc, mbrtoc32 and mbrlen stays taken in every UTF-8 locale. A per-call INSTRUCTION
 * CEILING turns a declining fast path red: the quick path costs a few dozen instructions per call, the
 * general decoder several times that, so a call that silently falls back to it exceeds the ceiling.
 *
 * Counting is exact and needs no PMU (the gate also runs in a 5.10 guest without one): a traced child
 * runs each case between two raise() markers and the parent single-steps it (PTRACE_SINGLESTEP), so
 * the count is (steps with K calls - steps with 0 calls) / K - the markers, loop setup and the
 * harness cancel; the loop's own bookkeeping (a few instructions per call) stays in. Every call's
 * result is also checked against the decoded value. Cases: ASCII and a two-byte character, with the
 * caller's state and with the hidden state, in C.UTF-8 and en_US.UTF-8 (en_US.UTF-8 compiled with
 * the host's localedef into a private LOCPATH directory when the host does not provide it). */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ptrace.h>
#include <sys/wait.h>
#include <uchar.h>
#include <unistd.h>
#include <wchar.h>

/* Per-call ceiling (instructions, loop bookkeeping included). Measured on keke (WL-HOT-SEQ4), the same in
 * both locales: quick path 48-54 (mbrlen) and 64-71 (mbrtowc, mbrtoc32); with the quick path planted
 * off, the general decoder costs 143-198 per call. */
#define CEILING 100
#define K 8

static size_t (*volatile f_towc)(wchar_t *, const char *, size_t, mbstate_t *) = mbrtowc;
static size_t (*volatile f_toc32)(char32_t *, const char *, size_t, mbstate_t *) = mbrtoc32;
static size_t (*volatile f_len)(const char *, size_t, mbstate_t *) = mbrlen;

static const char ascii[] = "abcdefgh";
static const char two[] = "\xc3\xa9\xc3\xa8xyz"; /* U+00E9 */

enum { TOWC_ASCII, TOWC_TWO, TOWC_TWO_HIDDEN, TOC32_TWO, LEN_ASCII, LEN_TWO, LEN_ASCII_HIDDEN, NCASES };
static const char *const names[NCASES] = {
    "mbrtowc ascii", "mbrtowc 2-byte", "mbrtowc 2-byte hidden", "mbrtoc32 2-byte",
    "mbrlen ascii", "mbrlen 2-byte", "mbrlen ascii hidden",
};
static const char *const locales[2] = {"C.UTF-8", "en_US.UTF-8"};
static char dir[256];

/* Runs `calls` calls of case c; returns the number of wrong results. */
static int body(int c, int calls)
{
    mbstate_t st;
    memset(&st, 0, sizeof st);
    wchar_t wc = 0;
    char32_t c32 = 0;
    int bad = 0;
    for (int i = 0; i < calls; i++) {
        size_t r;
        switch (c) {
        case TOWC_ASCII:
            r = f_towc(&wc, ascii + (i & 7), 8 - (i & 7), &st);
            bad += r != 1 || wc != (wchar_t)ascii[i & 7];
            break;
        case TOWC_TWO:
            r = f_towc(&wc, two, 7, &st);
            bad += r != 2 || wc != 0xe9;
            break;
        case TOWC_TWO_HIDDEN:
            r = f_towc(&wc, two, 7, NULL);
            bad += r != 2 || wc != 0xe9;
            break;
        case TOC32_TWO:
            r = f_toc32(&c32, two, 7, &st);
            bad += r != 2 || c32 != 0xe9;
            break;
        case LEN_ASCII:
            r = f_len(ascii + (i & 7), 8 - (i & 7), &st);
            bad += r != 1;
            break;
        case LEN_TWO:
            r = f_len(two, 7, &st);
            bad += r != 2;
            break;
        default:
            r = f_len(ascii + (i & 7), 8 - (i & 7), NULL);
            bad += r != 1;
            break;
        }
    }
    return bad;
}

/* The traced child: for each locale and case, one warm-up call, then a 0-call and a K-call region,
 * each between SIGUSR1 (start) and SIGUSR2 (end). Exit status: wrong results (0 = all right). */
static int child(void)
{
    if (ptrace(PTRACE_TRACEME, 0, NULL, NULL) != 0)
        return 100;
    raise(SIGSTOP);
    int bad = 0;
    for (int l = 0; l < 2; l++) {
        if (!setlocale(LC_ALL, locales[l]))
            return 101 + l;
        for (int c = 0; c < NCASES; c++) {
            bad += body(c, 1);
            for (int calls = 0; calls <= K; calls += K) {
                raise(SIGUSR1);
                bad += body(c, calls);
                raise(SIGUSR2);
            }
        }
    }
    return bad > 99 ? 99 : bad;
}

/* Single-steps the stopped child until it stops for SIGUSR2; returns the steps, -1 on error. */
static long steps_to_end(pid_t pid)
{
    long n = 0;
    for (;;) {
        int status;
        if (ptrace(PTRACE_SINGLESTEP, pid, NULL, NULL) != 0 || waitpid(pid, &status, 0) != pid || !WIFSTOPPED(status))
            return -1;
        if (WSTOPSIG(status) == SIGUSR2)
            return n;
        if (WSTOPSIG(status) != SIGTRAP)
            return -1;
        n++;
    }
}

/* Continues the child to its next SIGUSR1 stop (0), its exit (1, status in *code) or an error (-1). */
static int to_start(pid_t pid, int *code)
{
    for (;;) {
        int status;
        if (ptrace(PTRACE_CONT, pid, NULL, NULL) != 0 || waitpid(pid, &status, 0) != pid)
            return -1;
        if (WIFEXITED(status)) {
            *code = WEXITSTATUS(status);
            return 1;
        }
        if (!WIFSTOPPED(status))
            return -1;
        if (WSTOPSIG(status) == SIGUSR1)
            return 0;
    }
}

static void cleanup(void)
{
    if (dir[0]) {
        char cmd[300];
        snprintf(cmd, sizeof cmd, "rm -rf '%s'", dir);
        if (system(cmd) != 0)
            printf("could not remove %s\n", dir);
    }
}

/* en_US.UTF-8 from the host if it loads, else compiled privately (as in s04_casecmp_locales). */
static void prepare(void)
{
    if (setlocale(LC_ALL, "en_US.UTF-8")) {
        setlocale(LC_ALL, "C");
        return;
    }
    const char *tmp = getenv("TMPDIR");
    snprintf(dir, sizeof dir, "%s/s06-quick-XXXXXX", tmp && *tmp ? tmp : "/tmp");
    if (!mkdtemp(dir)) {
        dir[0] = 0;
        printf("FAIL: mkdtemp\n");
        exit(1);
    }
    atexit(cleanup);
    char cmd[600];
    snprintf(cmd, sizeof cmd,
             "cd '%s' && localedef -i en_US -f UTF-8 ./en_US.UTF-8 >/dev/null 2>&1;"
             " ln -s en_US.UTF-8 en_US.utf8; true", dir);
    if (system(cmd) == -1 || setenv("LOCPATH", dir, 1) != 0) {
        printf("FAIL: compiling en_US.UTF-8 (localedef from the locales package)\n");
        exit(1);
    }
}

int main(void)
{
    prepare();
    fflush(stdout);
    pid_t pid = fork();
    if (pid < 0) {
        printf("FAIL: fork\n");
        return 1;
    }
    if (pid == 0)
        _exit(child());
    int status;
    if (waitpid(pid, &status, 0) != pid || !WIFSTOPPED(status)) {
        printf("FAIL: the child did not stop for tracing\n");
        return 1;
    }
    int failures = 0, code = -1;
    for (int l = 0; l < 2; l++) {
        for (int c = 0; c < NCASES; c++) {
            long s[2];
            for (int k = 0; k < 2; k++) {
                int r = to_start(pid, &code);
                s[k] = r == 0 ? steps_to_end(pid) : -1;
                if (s[k] < 0) {
                    printf("FAIL: tracing %s %s (child exit %d)\n", locales[l], names[c], code);
                    if (r != 1)
                        kill(pid, SIGKILL);
                    waitpid(pid, &status, 0);
                    return 1;
                }
            }
            long per = (s[1] - s[0]) / K;
            int over = per > CEILING || s[1] <= s[0];
            failures += over;
            printf("%s %-12s %-24s %4ld instructions/call%s\n", over ? "FAIL:" : "ok   ", locales[l], names[c], per,
                   over ? " (over the ceiling: the quick path was not taken)" : "");
        }
    }
    if (to_start(pid, &code) != 1) {
        printf("FAIL: the child did not exit\n");
        kill(pid, SIGKILL);
        waitpid(pid, &status, 0);
        return 1;
    }
    if (code != 0) {
        printf("FAIL: the child reported %d (100: PTRACE_TRACEME, 101/102: setlocale, else wrong results)\n", code);
        failures++;
    }
    printf("CEILING_NOTE: ceiling %d instructions/call\n", CEILING);
    return failures != 0;
}
