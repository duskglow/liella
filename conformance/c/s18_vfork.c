/* Spec §18.2 (D-18.1, owner ruling #410): vfork shares the caller's memory, as glibc's does.
 * Checked: a child's write before _exit is seen by the parent (CLONE_VM); a child that execs
 * runs the program and its status comes back; a failed exec in the child ends it with 127;
 * after a vfork from a parent with 64 MiB of touched memory the parent rewrites it with almost
 * no page faults (no copy-on-write left behind); vfork blocks no signal, in the child or the
 * parent; a vfork refused by RLIMIT_NPROC returns -1 with EAGAIN in the parent. Failures are
 * printed and counted; the exit status is the count. */
#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

static int fails;
#define CHECK(c, what)                                              \
    do {                                                            \
        if (!(c)) {                                                 \
            fprintf(stderr, "FAIL line %d: %s\n", __LINE__, what);  \
            fails++;                                                \
        }                                                           \
    } while (0)

static int status_of(pid_t p)
{
    int st;
    if (p <= 0 || waitpid(p, &st, 0) != p || !WIFEXITED(st))
        return -1;
    return WEXITSTATUS(st);
}

static volatile int shared_word;

static long minflt(void)
{
    struct rusage ru;
    getrusage(RUSAGE_SELF, &ru);
    return ru.ru_minflt;
}

int main(void)
{
    /* The child's write is the parent's (one memory). */
    shared_word = 0;
    pid_t p = vfork();
    if (p == 0) {
        shared_word = 42;
        _exit(3);
    }
    CHECK(status_of(p) == 3, "vfork child _exit status");
    CHECK(shared_word == 42, "the child's write before _exit is seen by the parent (CLONE_VM)");

    /* A child that execs. */
    static char *const ok_argv[] = {"sh", "-c", "exit 6", NULL};
    static char *const no_env[] = {NULL};
    p = vfork();
    if (p == 0) {
        execve("/bin/sh", ok_argv, no_env);
        _exit(126);
    }
    CHECK(status_of(p) == 6, "vfork + execve runs the program");

    /* A failed exec: the child ends with 127, the parent resumes. */
    static char *const bad_argv[] = {"none", NULL};
    p = vfork();
    if (p == 0) {
        execve("/nonexistent/liella-vfork-test", bad_argv, no_env);
        _exit(127);
    }
    CHECK(status_of(p) == 127, "failed exec in the child: 127");

    /* No copy-on-write left behind: rewriting 64 MiB (4 KiB pages) after a vfork faults
     * (almost) nowhere; a copied address space faults once per page (16384). */
    size_t len = 64u << 20;
    unsigned char *big = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    CHECK(big != MAP_FAILED, "mmap 64 MiB");
    if (big != MAP_FAILED) {
        madvise(big, len, MADV_NOHUGEPAGE);
        memset(big, 1, len);
        p = vfork();
        if (p == 0)
            _exit(0);
        CHECK(status_of(p) == 0, "vfork from a large parent");
        long before = minflt();
        for (size_t i = 0; i < len; i += 4096)
            big[i] = 2;
        long faults = minflt() - before;
        if (faults >= 1024)
            fprintf(stderr, "rewrite after vfork: %ld minor faults\n", faults);
        CHECK(faults < 1024, "the parent's pages are not left copy-on-write");
        munmap(big, len);
    }

    /* No signal blocked: SIGUSR2 stays unblocked in the child (read with the raw syscall)
     * and in the parent; the parent's own blocked SIGUSR1 stays blocked. */
    sigset_t m;
    sigemptyset(&m);
    sigaddset(&m, SIGUSR1);
    sigprocmask(SIG_BLOCK, &m, NULL);
    p = vfork();
    if (p == 0) {
        unsigned long k = 0;
        syscall(SYS_rt_sigprocmask, SIG_BLOCK, NULL, &k, 8);
        unsigned long usr1 = 1ul << (SIGUSR1 - 1), usr2 = 1ul << (SIGUSR2 - 1);
        _exit((k & usr1) && !(k & usr2) ? 9 : 10);
    }
    CHECK(status_of(p) == 9, "the child's mask is the caller's (nothing blocked by vfork)");
    sigprocmask(SIG_BLOCK, NULL, &m);
    CHECK(sigismember(&m, SIGUSR1) && !sigismember(&m, SIGUSR2), "the parent's mask is unchanged");
    sigemptyset(&m);
    sigaddset(&m, SIGUSR1);
    sigprocmask(SIG_UNBLOCK, &m, NULL);

    /* RLIMIT_NPROC: in a helper process (as nobody when run as root, whom the limit spares),
     * vfork returns -1 with EAGAIN in the parent. */
    pid_t h = fork();
    if (h == 0) {
        if (geteuid() == 0 && (setgid(65534) != 0 || setuid(65534) != 0))
            _exit(20);
        struct rlimit rl = {0, 0};
        if (setrlimit(RLIMIT_NPROC, &rl) != 0)
            _exit(21);
        errno = 0;
        pid_t c = vfork();
        if (c == 0)
            _exit(0);
        if (c > 0) {
            waitpid(c, NULL, 0);
            _exit(22);
        }
        _exit(c == -1 && errno == EAGAIN ? 0 : 23);
    }
    int hs = status_of(h);
    if (hs != 0)
        fprintf(stderr, "RLIMIT_NPROC helper status %d\n", hs);
    CHECK(hs == 0, "vfork refused by RLIMIT_NPROC: -1, EAGAIN");
    return fails;
}
