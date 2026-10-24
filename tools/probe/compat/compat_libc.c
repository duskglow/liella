/* LEGACY-PROBE-COMPAT (LG-0b): the old (compat) versions of libc entries against the default ones.
 * Black-box: each old version is bound with .symver and called through a local name; only public
 * interfaces are called, and only observable results are printed (no addresses, pids or times).
 * Every output line starts with the versioned name it observes. Run by tools/probe/compat/run.sh
 * in a fresh scratch directory (relative paths below are inside it). */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <ftw.h>
#include <regex.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern void *memcpy_225(void *, const void *, size_t);
__asm__(".symver memcpy_225, memcpy@GLIBC_2.2.5");
extern FILE *fmemopen_225(void *, size_t, const char *);
__asm__(".symver fmemopen_225, fmemopen@GLIBC_2.2.5");
extern char *realpath_225(const char *, char *);
__asm__(".symver realpath_225, realpath@GLIBC_2.2.5");
extern int regexec_225(const regex_t *, const char *, size_t, regmatch_t *, int);
__asm__(".symver regexec_225, regexec@GLIBC_2.2.5");
typedef int (*nftw_cb)(const char *, const struct stat *, int, struct FTW *);
extern int nftw_225(const char *, nftw_cb, int, int);
__asm__(".symver nftw_225, nftw@GLIBC_2.2.5");
extern int nftw64_225(const char *, nftw_cb, int, int);
__asm__(".symver nftw64_225, nftw64@GLIBC_2.2.5");
extern void quick_exit_210(int) __attribute__((noreturn));
__asm__(".symver quick_exit_210, quick_exit@GLIBC_2.10");
extern int posix_spawn_225(pid_t *, const char *, const posix_spawn_file_actions_t *, const posix_spawnattr_t *, char *const[], char *const[]);
__asm__(".symver posix_spawn_225, posix_spawn@GLIBC_2.2.5");
extern int posix_spawnp_225(pid_t *, const char *, const posix_spawn_file_actions_t *, const posix_spawnattr_t *, char *const[], char *const[]);
__asm__(".symver posix_spawnp_225, posix_spawnp@GLIBC_2.2.5");
extern int clock_getres_225(clockid_t, struct timespec *);
__asm__(".symver clock_getres_225, clock_getres@GLIBC_2.2.5");
extern int clock_gettime_225(clockid_t, struct timespec *);
__asm__(".symver clock_gettime_225, clock_gettime@GLIBC_2.2.5");
extern int clock_settime_225(clockid_t, const struct timespec *);
__asm__(".symver clock_settime_225, clock_settime@GLIBC_2.2.5");
extern int clock_nanosleep_225(clockid_t, int, const struct timespec *, struct timespec *);
__asm__(".symver clock_nanosleep_225, clock_nanosleep@GLIBC_2.2.5");
extern int clock_getcpuclockid_225(pid_t, clockid_t *);
__asm__(".symver clock_getcpuclockid_225, clock_getcpuclockid@GLIBC_2.2.5");
extern int __cxa_thread_atexit_impl(void (*)(void *), void *, void *);
extern void *__dso_handle;

static const char *en(int e) { const char *s = strerrorname_np(e); return e == 0 ? "0" : s ? s : "?"; }

static void hex(const unsigned char *p, size_t n) {
    for (size_t i = 0; i < n; i++) printf("%02x", p[i]);
}

/* ---- memcpy@GLIBC_2.2.5: overlapping copies compared with memmove ---- */
static unsigned char mbuf[2][24000];
static void probe_memcpy(void) {
    static const int ns[] = {0, 1, 2, 3, 7, 8, 15, 16, 31, 32, 33, 63, 64, 100, 128, 255, 256, 1000, 4096, 10000};
    static const int ds[] = {1, -1, 3, -3, 8, -8, 15, -15, 16, -16, 64, -64, 1000, -1000};
    for (size_t a = 0; a < sizeof ns / sizeof *ns; a++) {
        printf("memcpy@GLIBC_2.2.5 overlap n=%d: shifts unlike memmove:", ns[a]);
        int bad = 0;
        for (size_t b = 0; b < sizeof ds / sizeof *ds; b++) {
            for (int k = 0; k < 2; k++)
                for (size_t i = 0; i < sizeof mbuf[0]; i++) mbuf[k][i] = (unsigned char)(i * 7 + 3);
            unsigned char *s0 = mbuf[0] + 12000, *s1 = mbuf[1] + 12000;
            memcpy_225(s0 + ds[b], s0, (size_t)ns[a]);
            memmove(s1 + ds[b], s1, (size_t)ns[a]);
            if (memcmp(mbuf[0], mbuf[1], sizeof mbuf[0]) != 0) { printf(" %+d", ds[b]); bad = 1; }
        }
        printf(bad ? "\n" : " none\n");
    }
}

/* ---- fmemopen@GLIBC_2.2.5 against fmemopen@@GLIBC_2.22 ---- */
typedef FILE *(*fmo_t)(void *, size_t, const char *);
static void fm_cases(const char *tag, fmo_t fo) {
    char buf[16], rd[32];
    FILE *f;
    errno = 0;
    f = fo(buf, 0, "r");
    printf("%s size0 r: %s errno=%s\n", tag, f ? "FILE" : "NULL", en(f ? 0 : errno));
    if (f) fclose(f);
    errno = 0;
    f = fo(NULL, 0, "w+");
    printf("%s NULL size0 w+: %s errno=%s\n", tag, f ? "FILE" : "NULL", en(f ? 0 : errno));
    if (f) fclose(f);

    memset(buf, 0, sizeof buf); strcpy(buf, "hello");
    f = fo(buf, 16, "r");
    if (f) {
        size_t n = fread(rd, 1, sizeof rd, f);
        printf("%s r \"hello\" size16: fread=%zu feof=%d\n", tag, n, feof(f));
        fseek(f, 0, SEEK_END);
        printf("%s r \"hello\" size16: SEEK_END ftell=%ld\n", tag, ftell(f));
        printf("%s r size16: fseek 17=%d fseek 16=%d\n", tag, fseek(f, 17, SEEK_SET), fseek(f, 16, SEEK_SET));
        fclose(f);
    }
    memset(buf, 0, sizeof buf); strcpy(buf, "hello");
    f = fo(buf, 16, "rb");
    if (f) { fseek(f, 0, SEEK_END); printf("%s rb \"hello\" size16: SEEK_END ftell=%ld\n", tag, ftell(f)); fclose(f); }

    memset(buf, 'x', sizeof buf);
    f = fo(buf, 8, "w");
    if (f) {
        printf("%s w size8 at open: buf=", tag); hex((unsigned char *)buf, 9); printf("\n");
        fputs("abc", f); fflush(f);
        printf("%s w size8 \"abc\" fflush: buf=", tag); hex((unsigned char *)buf, 9); printf(" ftell=%ld\n", ftell(f));
        fseek(f, 1, SEEK_SET); fflush(f);
        printf("%s w size8 seek1 fflush: buf=", tag); hex((unsigned char *)buf, 9); printf("\n");
        fclose(f);
        printf("%s w size8 fclose: buf=", tag); hex((unsigned char *)buf, 9); printf("\n");
    }
    memset(buf, 'x', sizeof buf);
    f = fo(buf, 4, "w");
    if (f) {
        int r = fputs("abcdef", f), fl = fflush(f);
        printf("%s w size4 \"abcdef\": fputs=%d fflush=%d ferror=%d buf=", tag, r, fl, ferror(f)); hex((unsigned char *)buf, 5); printf("\n");
        fclose(f);
        printf("%s w size4 fclose: buf=", tag); hex((unsigned char *)buf, 5); printf("\n");
    }
    memset(buf, 'x', sizeof buf); memcpy(buf, "ab\0yyyy", 8);
    f = fo(buf, 8, "a");
    if (f) {
        printf("%s a \"ab\" size8: ftell at open=%ld\n", tag, ftell(f));
        fputs("C", f); fflush(f);
        printf("%s a \"ab\" size8 \"C\": buf=", tag); hex((unsigned char *)buf, 9); printf("\n");
        fseek(f, 0, SEEK_SET); fputs("D", f); fflush(f);
        printf("%s a seek0 \"D\": buf=", tag); hex((unsigned char *)buf, 9); printf("\n");
        fclose(f);
    }
    memset(buf, 'x', sizeof buf); memcpy(buf, "ab\0yyyy", 8);
    f = fo(buf, 8, "a+");
    if (f) { int c = fgetc(f); printf("%s a+ \"ab\" size8: ftell=%ld first fgetc=%d\n", tag, ftell(f), c); fclose(f); }
    f = fo(NULL, 8, "w+");
    if (f) {
        fputs("abc", f); rewind(f);
        size_t n = fread(rd, 1, sizeof rd, f);
        printf("%s NULL size8 w+ \"abc\" rewind: fread=%zu first3=", tag, n); hex((unsigned char *)rd, n < 3 ? n : 3); printf("\n");
        fclose(f);
    }
    memset(buf, 'x', sizeof buf); strcpy(buf, "hello");
    f = fo(buf, 16, "r+");
    if (f) {
        fseek(f, 0, SEEK_END); long e = ftell(f);
        fputs("Z", f); fflush(f);
        printf("%s r+ \"hello\" size16: SEEK_END ftell=%ld then \"Z\" buf=", tag, e); hex((unsigned char *)buf, 17); printf("\n");
        fclose(f);
    }
}

/* ---- realpath@GLIBC_2.2.5 ---- */
static void rp_case(const char *what, const char *path, int withbuf) {
    char b0[4096], b1[4096];
    errno = 0; char *r0 = realpath_225(path, withbuf ? b0 : NULL); int e0 = errno;
    errno = 0; char *r1 = realpath(path, withbuf ? b1 : NULL); int e1 = errno;
    printf("realpath@GLIBC_2.2.5 %s %s: %s errno=%s | default: %s errno=%s%s\n", what, withbuf ? "buf" : "NULL",
           r0 ? "ok" : "NULL", en(r0 ? 0 : e0), r1 ? "ok" : "NULL", en(r1 ? 0 : e1),
           r0 && r1 ? (strcmp(r0, r1) ? " results differ" : " results equal") : "");
    if (r0 && r0 != b0) free(r0);
    if (r1 && r1 != b1) free(r1);
}
static void probe_realpath(void) {
    for (int w = 0; w < 2; w++) {
        rp_case("\"/\"", "/", w);
        rp_case("\".\"", ".", w);
        rp_case("\"\"", "", w);
        rp_case("\"/nonexistent-liella/x\"", "/nonexistent-liella/x", w);
        rp_case("\"rp/loop\"", "rp/loop", w);
        rp_case("\"rp/file/\"", "rp/file/", w);
        rp_case("\"rp/dangling\"", "rp/dangling", w);
    }
    char b[4096];
    errno = 0; char *r = realpath_225(NULL, b);
    printf("realpath@GLIBC_2.2.5 NULL path: %s errno=%s", r ? "ok" : "NULL", en(r ? 0 : errno));
    errno = 0; r = realpath(NULL, b);
    printf(" | default: %s errno=%s\n", r ? "ok" : "NULL", en(r ? 0 : errno));
}

/* ---- regexec@GLIBC_2.2.5 ---- */
typedef int (*rx_t)(const regex_t *, const char *, size_t, regmatch_t *, int);
static void rx_case(const char *what, const char *pat, int cfl, const char *s, size_t nm, int so, int eo, int efl) {
    rx_t fns[2] = {regexec_225, regexec};
    regex_t re;
    if (regcomp(&re, pat, cfl) != 0) { printf("regexec@GLIBC_2.2.5 %s: regcomp failed\n", what); return; }
    printf("regexec@GLIBC_2.2.5 %s:", what);
    for (int k = 0; k < 2; k++) {
        regmatch_t m[3];
        for (int i = 0; i < 3; i++) m[i].rm_so = m[i].rm_eo = -7;
        m[0].rm_so = so; m[0].rm_eo = eo;
        int r = fns[k](&re, s, nm, m, efl);
        printf("%s ret=%d m=", k ? " | default:" : "", r);
        for (int i = 0; i < 3; i++) printf("%s(%d,%d)", i ? "," : "", (int)m[i].rm_so, (int)m[i].rm_eo);
    }
    printf("\n");
    regfree(&re);
}
static void probe_regexec(void) {
    rx_case("\"b\" in \"abcb\" STARTEND [1,3) nmatch1", "b", REG_EXTENDED, "abcb", 1, 1, 3, REG_STARTEND);
    rx_case("\"b\" in \"abcb\" STARTEND [2,4) nmatch1", "b", REG_EXTENDED, "abcb", 1, 2, 4, REG_STARTEND);
    rx_case("\"b\" in \"abcb\" STARTEND [2,3) nmatch1", "b", REG_EXTENDED, "abcb", 1, 2, 3, REG_STARTEND);
    rx_case("\"b\" in \"abcb\" STARTEND [1,3) nmatch0", "b", REG_EXTENDED, "abcb", 0, 1, 3, REG_STARTEND);
    rx_case("\"^c\" in \"abc\" STARTEND [2,3)", "^c", REG_EXTENDED, "abc", 1, 2, 3, REG_STARTEND);
    rx_case("\"^c\" in \"abc\" STARTEND|NOTBOL [2,3)", "^c", REG_EXTENDED, "abc", 1, 2, 3, REG_STARTEND | REG_NOTBOL);
    rx_case("\"a\\0b\" STARTEND [0,3) nul inside", "b", REG_EXTENDED, "a\0b", 1, 0, 3, REG_STARTEND);
    rx_case("\"(a)(b)?\" in \"a\" nmatch3", "(a)(b)?", REG_EXTENDED, "a", 3, 0, 0, 0);
    rx_case("\"(a)\" NOSUB in \"xa\" nmatch3", "(a)", REG_EXTENDED | REG_NOSUB, "xa", 3, 0, 0, 0);
    rx_case("\"^a\" NOTBOL in \"abc\"", "^a", REG_EXTENDED, "abc", 1, 0, 0, REG_NOTBOL);
    rx_case("\"c$\" NOTEOL in \"abc\"", "c$", REG_EXTENDED, "abc", 1, 0, 0, REG_NOTEOL);
    rx_case("\"a\" eflags 0x80 unknown", "a", REG_EXTENDED, "a", 1, 0, 0, 0x80);
}

/* ---- nftw@GLIBC_2.2.5, nftw64@GLIBC_2.2.5 ---- */
static char visits[64][128];
static int nvis, cbret_mode;
static int cb(const char *p, const struct stat *st, int tf, struct FTW *fw) {
    (void)st;
    if (nvis < 64) snprintf(visits[nvis++], sizeof visits[0], "%s:%d:%d:%d", p, tf, fw->level, fw->base);
    if (cbret_mode == 1 && strcmp(p, "t/d") == 0) return FTW_SKIP_SUBTREE;
    if (cbret_mode == 2 && strcmp(p, "t/d") == 0) return FTW_SKIP_SIBLINGS;
    if (cbret_mode == 3) return FTW_STOP;
    if (cbret_mode == 4) return 77;
    return 0;
}
static int cmpv(const void *a, const void *b) { return strcmp((const char *)a, (const char *)b); }
typedef int (*nftw_t)(const char *, nftw_cb, int, int);
static void nftw_case(const char *what, const char *root, int fd, int flags, int mode) {
    static const char *names[3] = {"nftw@GLIBC_2.2.5", "nftw64@GLIBC_2.2.5", "nftw@GLIBC_2.2.5 default"};
    nftw_t fns[3] = {nftw_225, nftw64_225, nftw};
    for (int k = 0; k < 3; k++) {
        nvis = 0; cbret_mode = mode; errno = 0;
        int r = fns[k](root, cb, fd, flags);
        int e = errno;
        qsort(visits, (size_t)nvis, sizeof visits[0], cmpv);
        printf("%s %s: ret=%d errno=%s visits(sorted)=", k == 2 ? "nftw@GLIBC_2.2.5" : names[k], what, r, en(r == -1 ? e : 0));
        if (k == 2) printf("[default] ");
        for (int i = 0; i < nvis; i++) printf("%s%s", i ? " " : "", visits[i]);
        printf("\n");
    }
}
static void probe_nftw(void) {
    nftw_case("flags=0", "t", 8, 0, 0);
    nftw_case("PHYS", "t", 8, FTW_PHYS, 0);
    nftw_case("PHYS|DEPTH", "t", 8, FTW_PHYS | FTW_DEPTH, 0);
    nftw_case("PHYS|MOUNT|CHDIR", "t", 8, FTW_PHYS | FTW_MOUNT | FTW_CHDIR, 0);
    nftw_case("flags=0x20 unknown", "t", 8, 0x20, 0);
    nftw_case("flags=0x100 unknown", "t", 8, 0x100, 0);
    nftw_case("flags=0x40000000 unknown", "t", 8, 0x40000000, 0);
    nftw_case("ACTIONRETVAL skip-subtree at t/d", "t", 8, FTW_ACTIONRETVAL | FTW_PHYS, 1);
    /* FTW_SKIP_SIBLINGS is not probed: what it skips depends on directory order. */
    nftw_case("ACTIONRETVAL stop", "t", 8, FTW_ACTIONRETVAL | FTW_PHYS, 3);
    nftw_case("ACTIONRETVAL ret77", "t", 8, FTW_ACTIONRETVAL | FTW_PHYS, 4);
    nftw_case("callback ret77 no ACTIONRETVAL", "t", 8, FTW_PHYS, 4);
    nftw_case("nopenfd=0", "t", 0, FTW_PHYS, 0);
    nftw_case("nopenfd=-1", "t", -1, FTW_PHYS, 0);
    nftw_case("missing root", "nonexistent-liella", 8, FTW_PHYS, 0);
    nftw_case("empty root", "", 8, FTW_PHYS, 0);
}

/* ---- quick_exit@GLIBC_2.10 against quick_exit@@GLIBC_2.24 ---- */
static void qh1(void) { write(1, "[qh1]", 5); }
static void qh2(void) { write(1, "[qh2]", 5); }
static void ah(void) { write(1, "[atexit]", 8); }
static void tld(void *p) { (void)p; write(1, "[tls-dtor]", 10); }
static void qe_case(int old, int tls) {
    int fds[2];
    if (pipe(fds) != 0) return;
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        dup2(fds[1], 1); close(fds[0]); close(fds[1]);
        at_quick_exit(qh1); at_quick_exit(qh2); atexit(ah);
        if (tls) __cxa_thread_atexit_impl(tld, NULL, &__dso_handle);
        setvbuf(stdout, NULL, _IOFBF, 4096);
        printf("[stdio-buffered]");
        if (old) quick_exit_210(3); else quick_exit(3);
    }
    close(fds[1]);
    char out[256]; ssize_t n, t = 0;
    while ((n = read(fds[0], out + t, sizeof out - 1 - (size_t)t)) > 0) t += n;
    out[t] = 0; close(fds[0]);
    int st; waitpid(pid, &st, 0);
    printf("quick_exit@GLIBC_2.10 %s%s: output=\"%s\" status=%s%d\n", old ? "" : "[default] ", tls ? "with thread_local dtor" : "plain",
           out, WIFEXITED(st) ? "exit " : "signal ", WIFEXITED(st) ? WEXITSTATUS(st) : WTERMSIG(st));
}

/* ---- posix_spawn@GLIBC_2.2.5, posix_spawnp@GLIBC_2.2.5 ---- */
typedef int (*spawn_t)(pid_t *, const char *, const posix_spawn_file_actions_t *, const posix_spawnattr_t *, char *const[], char *const[]);
static void sp_case(const char *what, int p, const char *file, int badopen) {
    spawn_t fns[2] = {p ? posix_spawnp_225 : posix_spawn_225, p ? posix_spawnp : posix_spawn};
    char *argv[] = {(char *)file, (char *)"-c", (char *)"exit 7", NULL};
    char *envp[] = {(char *)"PATH=/nonexistent-liella:spawn:/bin:/usr/bin", NULL};
    printf("%s %s:", p ? "posix_spawnp@GLIBC_2.2.5" : "posix_spawn@GLIBC_2.2.5", what);
    for (int k = 0; k < 2; k++) {
        posix_spawn_file_actions_t fa; posix_spawn_file_actions_init(&fa);
        if (badopen) posix_spawn_file_actions_addopen(&fa, 5, "/nonexistent-liella/x", O_RDONLY, 0);
        char *oldpath = getenv("PATH") ? strdup(getenv("PATH")) : NULL;
        setenv("PATH", "/nonexistent-liella:spawn:/bin:/usr/bin", 1);
        pid_t pid = -1;
        int r = fns[k](&pid, file, &fa, NULL, argv, envp);
        if (oldpath) { setenv("PATH", oldpath, 1); free(oldpath); }
        posix_spawn_file_actions_destroy(&fa);
        printf("%s ret=%s", k ? " | default:" : "", en(r));
        if (r == 0) {
            int st; waitpid(pid, &st, 0);
            if (WIFEXITED(st)) printf(" child exit %d", WEXITSTATUS(st)); else printf(" child signal %d", WTERMSIG(st));
        }
    }
    printf("\n");
}
static void probe_spawn(void) {
    sp_case("\"/bin/sh -c 'exit 7'\"", 0, "/bin/sh", 0);
    sp_case("missing file", 0, "/nonexistent-liella/prog", 0);
    sp_case("not executable", 0, "spawn/noexec", 0);
    sp_case("no #! script", 0, "spawn/script", 0);
    sp_case("bad addopen", 0, "/bin/sh", 1);
    sp_case("\"sh\" on PATH", 1, "sh", 0);
    sp_case("missing on PATH", 1, "nonexistent-liella-cmd", 0);
    sp_case("not executable on PATH", 1, "noexec", 0);
    sp_case("no #! script on PATH", 1, "script", 0);
    sp_case("bad addopen", 1, "sh", 1);
}

/* ---- clock_*@GLIBC_2.2.5 ---- */
static void probe_clock(void) {
    struct timespec a, b;
    static const clockid_t ids[] = {CLOCK_REALTIME, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_THREAD_CPUTIME_ID, CLOCK_MONOTONIC_RAW, CLOCK_BOOTTIME, -5, 99};
    for (size_t i = 0; i < sizeof ids / sizeof *ids; i++) {
        errno = 0; int r0 = clock_getres_225(ids[i], &a); int e0 = errno;
        errno = 0; int r1 = clock_getres(ids[i], &b); int e1 = errno;
        printf("clock_getres@GLIBC_2.2.5 id=%d: ret=%d errno=%s | default: ret=%d errno=%s%s\n", (int)ids[i], r0, en(r0 ? e0 : 0), r1, en(r1 ? e1 : 0),
               r0 == 0 && r1 == 0 ? (a.tv_sec == b.tv_sec && a.tv_nsec == b.tv_nsec ? " res equal" : " res differ") : "");
        errno = 0; r0 = clock_gettime_225(ids[i], &a); e0 = errno;
        errno = 0; r1 = clock_gettime(ids[i], &b); e1 = errno;
        int order = r0 == 0 && r1 == 0 && (b.tv_sec > a.tv_sec || (b.tv_sec == a.tv_sec && b.tv_nsec >= a.tv_nsec));
        printf("clock_gettime@GLIBC_2.2.5 id=%d: ret=%d errno=%s | default: ret=%d errno=%s%s\n", (int)ids[i], r0, en(r0 ? e0 : 0), r1, en(r1 ? e1 : 0),
               r0 == 0 && r1 == 0 ? (order || ids[i] == CLOCK_REALTIME ? " same clock" : " ORDER") : "");
    }
    clock_gettime(CLOCK_REALTIME, &a);
    static const clockid_t sids[] = {CLOCK_REALTIME, CLOCK_MONOTONIC, -5};
    for (size_t i = 0; i < 3; i++) {
        errno = 0; int r0 = clock_settime_225(sids[i], &a); int e0 = errno;
        errno = 0; int r1 = clock_settime(sids[i], &a); int e1 = errno;
        printf("clock_settime@GLIBC_2.2.5 id=%d (unprivileged): ret=%d errno=%s | default: ret=%d errno=%s\n", (int)sids[i], r0, en(r0 ? e0 : 0), r1, en(r1 ? e1 : 0));
    }
    struct timespec bad = {0, 2000000000L}, neg = {-1, 0}, tiny = {0, 1000};
    struct { const char *w; clockid_t id; int fl; struct timespec *t; } ns[] = {
        {"MONOTONIC 1us", CLOCK_MONOTONIC, 0, &tiny}, {"REALTIME ABSTIME past", CLOCK_REALTIME, TIMER_ABSTIME, &tiny},
        {"nsec=2e9", CLOCK_MONOTONIC, 0, &bad}, {"sec=-1", CLOCK_MONOTONIC, 0, &neg}, {"THREAD_CPUTIME", CLOCK_THREAD_CPUTIME_ID, 0, &tiny},
        {"PROCESS_CPUTIME ABSTIME past", CLOCK_PROCESS_CPUTIME_ID, TIMER_ABSTIME, &tiny}, {"id=-5", -5, 0, &tiny}, {"id=99", 99, 0, &tiny}};
    for (size_t i = 0; i < sizeof ns / sizeof *ns; i++) {
        errno = 0; int r0 = clock_nanosleep_225(ns[i].id, ns[i].fl, ns[i].t, NULL); int e0 = errno;
        errno = 0; int r1 = clock_nanosleep(ns[i].id, ns[i].fl, ns[i].t, NULL); int e1 = errno;
        printf("clock_nanosleep@GLIBC_2.2.5 %s: ret=%s errno-after=%s | default: ret=%s errno-after=%s\n", ns[i].w, en(r0), en(e0), en(r1), en(e1));
    }
    static const pid_t pids[] = {0, 1, 0x7ffffff0};
    for (size_t i = 0; i < 3; i++) {
        clockid_t c0 = 12345, c1 = 12345;
        int r0 = clock_getcpuclockid_225(pids[i] == 0 ? 0 : pids[i], &c0);
        int r1 = clock_getcpuclockid(pids[i], &c1);
        printf("clock_getcpuclockid@GLIBC_2.2.5 pid=%s: ret=%s | default: ret=%s%s\n", i == 0 ? "0" : i == 1 ? "1" : "nonexistent", en(r0), en(r1),
               r0 == 0 && r1 == 0 ? (c0 == c1 ? " ids equal" : " ids differ") : "");
    }
    clockid_t c = 0;
    if (clock_getcpuclockid(getpid(), &c) == 0) {
        clockid_t c0 = 0;
        int r0 = clock_getcpuclockid_225(getpid(), &c0);
        printf("clock_getcpuclockid@GLIBC_2.2.5 pid=self: ret=%s %s\n", en(r0), r0 == 0 && c0 == c ? "id equals default" : "id differs");
    }
}

static void setup(void) {
    mkdir("t", 0755); mkdir("t/d", 0755); mkdir("t/d/e", 0755);
    close(open("t/d/e/f", O_CREAT | O_WRONLY, 0644));
    symlink("e/f", "t/d/l"); symlink("nowhere", "t/d/dl");
    mkdir("t/x", 0755); mkdir("t/u", 0); /* unreadable directory (as non-root) */
    mkdir("rp", 0755); symlink("loop", "rp/loop"); close(open("rp/file", O_CREAT | O_WRONLY, 0644)); symlink("nowhere", "rp/dangling");
    mkdir("spawn", 0755);
    close(open("spawn/noexec", O_CREAT | O_WRONLY, 0644));
    int fd = open("spawn/script", O_CREAT | O_WRONLY, 0755);
    if (fd >= 0) { write(fd, "exit 9\n", 7); close(fd); }
}

int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    setup();
    probe_memcpy();
    fm_cases("fmemopen@GLIBC_2.2.5", fmemopen_225);
    fm_cases("fmemopen@GLIBC_2.2.5 [default]", fmemopen);
    probe_realpath();
    probe_regexec();
    probe_nftw();
    qe_case(1, 0); qe_case(0, 0); qe_case(1, 1); qe_case(0, 1);
    probe_spawn();
    probe_clock();
    chmod("t/u", 0755);
    return 0;
}
