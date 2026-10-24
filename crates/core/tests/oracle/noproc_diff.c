/* The functions that read /proc or /sys, run with and without them (OD-5; plan "Without /proc").
 *
 * Black-box probe: public interfaces only. Each line names a function and what it answered:
 * a value, or -1/NULL with errno. The harness (noproc_products.rs) runs it twice: normally,
 * and in a private user and mount namespace with an empty tmpfs mounted over /proc and over
 * /sys (`unshare -r -m`, then `mount -t tmpfs none /proc`), so that every path below them is
 * missing (ENOENT). The first line says which.
 *
 * Values that differ between machines are printed as relations: the processor counts are
 * compared with the affinity mask's count, the stack size with RLIMIT_STACK, names are shown
 * only as "set" or "empty".
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <pthread.h>
#include <pwd.h>
#include <sched.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/sysinfo.h>
#include <sys/wait.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

extern int pidfd_getpid(int) __attribute__((weak));

static const char *en(int e)
{
    switch (e) {
    case 0: return "0";
    case ENOENT: return "ENOENT";
    case ENOMEM: return "ENOMEM";
    case EBADF: return "EBADF";
    case ENODEV: return "ENODEV";
    case ENOTTY: return "ENOTTY";
    case ENXIO: return "ENXIO";
    case EINVAL: return "EINVAL";
    case ERANGE: return "ERANGE";
    case ESRCH: return "ESRCH";
    case ENOSYS: return "ENOSYS";
    case EACCES: return "EACCES";
    case EPERM: return "EPERM";
    case EOPNOTSUPP: return "EOPNOTSUPP";
    default: {
        static char b[32];
        snprintf(b, sizeof b, "errno %d", e);
        return b;
    }
    }
}

static int affinity_count(void)
{
    cpu_set_t s;
    if (sched_getaffinity(0, sizeof s, &s) != 0)
        return -1;
    return CPU_COUNT(&s);
}

static const char *rel(long v, long ref)
{
    return v == ref ? "= affinity" : v > ref ? "> affinity" : "< affinity";
}

static void *idle(void *arg)
{
    (void)arg;
    pause();
    return NULL;
}

int main(void)
{
    struct stat st;
    printf("/proc present: %s; /sys present: %s\n", stat("/proc/self", &st) == 0 ? "yes" : "no", stat("/sys/devices", &st) == 0 ? "yes" : "no");
    int aff = affinity_count();

    /* §24.4 processor counts. */
    printf("get_nprocs: %s\n", rel(get_nprocs(), aff));
    printf("get_nprocs_conf: %s\n", get_nprocs_conf() >= get_nprocs() ? ">= get_nprocs" : "< get_nprocs");
    printf("sysconf(_SC_NPROCESSORS_ONLN): %s\n", rel(sysconf(_SC_NPROCESSORS_ONLN), aff));
    printf("sysconf(_SC_NPROCESSORS_CONF) >= ONLN: %s\n", sysconf(_SC_NPROCESSORS_CONF) >= sysconf(_SC_NPROCESSORS_ONLN) ? "yes" : "no");
    errno = 0;
    printf("sysconf(_SC_NGROUPS_MAX): %ld errno=%s\n", sysconf(_SC_NGROUPS_MAX), en(errno));
    printf("sysconf(_SC_PHYS_PAGES) > 0: %s\n", sysconf(_SC_PHYS_PAGES) > 0 ? "yes" : "no");
    printf("sysconf(_SC_PAGESIZE): %ld\n", sysconf(_SC_PAGESIZE));
    printf("sysconf(_SC_CLK_TCK): %ld\n", sysconf(_SC_CLK_TCK));

    /* §1.5 auxiliary vector, §23.1 clocks (the vDSO is found through it). */
    printf("getauxval(AT_PAGESZ): %lu; AT_SYSINFO_EHDR set: %s\n", getauxval(AT_PAGESZ), getauxval(AT_SYSINFO_EHDR) ? "yes" : "no");
    struct timespec ts;
    printf("clock_gettime(CLOCK_MONOTONIC): %d\n", clock_gettime(CLOCK_MONOTONIC, &ts));

    /* §13.10 pathconf: _PC_LINK_MAX on the working directory's file system. */
    errno = 0;
    long lm = pathconf(".", _PC_LINK_MAX);
    printf("pathconf(., _PC_LINK_MAX): %ld errno=%s\n", lm, en(errno));

    /* §22.2 pthread_getattr_np of the initial thread. */
    pthread_attr_t a;
    int r = pthread_getattr_np(pthread_self(), &a);
    if (r == 0) {
        void *addr;
        size_t size;
        pthread_attr_getstack(&a, &addr, &size);
        struct rlimit rl;
        getrlimit(RLIMIT_STACK, &rl);
        printf("pthread_getattr_np(main): 0, stack size %s\n", size == 0 ? "0" : (rl.rlim_cur != RLIM_INFINITY && size <= rl.rlim_cur) ? "<= RLIMIT_STACK" : "> RLIMIT_STACK");
        pthread_attr_destroy(&a);
    } else {
        printf("pthread_getattr_np(main): %s\n", en(r));
    }

    /* §22.10 thread names of another thread (the calling thread's own go through prctl). */
    pthread_t t;
    pthread_create(&t, NULL, idle, NULL);
    char name[32];
    r = pthread_setname_np(t, "worker");
    printf("pthread_setname_np(other): %s\n", en(r));
    r = pthread_getname_np(t, name, sizeof name);
    printf("pthread_getname_np(other): %s%s\n", en(r), r == 0 ? (strcmp(name, "worker") == 0 ? " worker" : " (other name)") : "");
    r = pthread_setname_np(pthread_self(), "probe");
    printf("pthread_setname_np(self): %s\n", en(r));
    r = pthread_getname_np(pthread_self(), name, sizeof name);
    printf("pthread_getname_np(self): %s %s\n", en(r), r == 0 ? name : "");

    /* §18.5 pidfd_getpid. */
    pid_t c = fork();
    if (c == 0) {
        pause();
        _exit(0);
    }
    int pfd = (int)syscall(SYS_pidfd_open, c, 0);
    if (pidfd_getpid) {
        errno = 0;
        int p = pidfd_getpid(pfd);
        printf("pidfd_getpid(pidfd): %s errno=%s\n", p == c ? "the child" : p == -1 ? "-1" : "another pid", en(errno));
        errno = 0;
        p = pidfd_getpid(1000);
        printf("pidfd_getpid(closed descriptor): %d errno=%s\n", p, en(errno));
        int nfd = open("/dev/null", O_RDONLY);
        errno = 0;
        p = pidfd_getpid(nfd);
        printf("pidfd_getpid(not a pidfd): %d errno=%s\n", p, en(errno));
        close(nfd);
    }
    kill(c, SIGKILL);
    waitpid(c, NULL, 0);
    close(pfd);

    /* §19.3 terminal names, §17.5 login name and slot, on a pseudo-terminal. */
    int m = posix_openpt(O_RDWR | O_NOCTTY);
    grantpt(m);
    unlockpt(m);
    int s = open(ptsname(m), O_RDWR | O_NOCTTY);
    errno = 0;
    char *tn = ttyname(s);
    printf("ttyname(pty): %s errno=%s\n", tn ? (strncmp(tn, "/dev/pts/", 9) == 0 ? "/dev/pts/N" : "other") : "NULL", en(errno));
    char buf[64];
    r = ttyname_r(s, buf, sizeof buf);
    printf("ttyname_r(pty): %s\n", en(r));
    errno = 0;
    tn = ttyname(m);
    printf("ttyname(master): %s errno=%s\n", tn ? "a name" : "NULL", en(errno));
    errno = 0;
    tn = ttyname(open("/dev/null", O_RDONLY));
    printf("ttyname(/dev/null): %s errno=%s\n", tn ? "a name" : "NULL", en(errno));
    printf("isatty(pty): %d\n", isatty(s));
    int saved = dup(0);
    dup2(s, 0);
    errno = 0;
    char *ln = getlogin();
    printf("getlogin (stdin a pty without a utmp record): %s errno=%s\n", ln ? "a name" : "NULL", en(errno));
    r = getlogin_r(buf, sizeof buf);
    printf("getlogin_r: %s\n", r == 0 ? "a name" : en(r));
    errno = 0;
    printf("ttyslot: %d\n", ttyslot());
    dup2(saved, 0);
    close(saved);

    /* §17.1 group_member (more than 64 groups would use /proc; with fewer, getgroups). */
    printf("group_member(getegid()): %d\n", group_member(getegid()));

    /* §11.2 closefrom. */
    int hi = dup2(1, 200);
    closefrom(150);
    printf("closefrom(150): descriptor 200 %s\n", fcntl(hi, F_GETFD) == -1 && errno == EBADF ? "closed" : "open");

    /* §13.7 lchmod (fchmodat2 on Linux 6.6 and later; /proc on older kernels). */
    char tmpl[] = "noproc-XXXXXX";
    int f = mkstemp(tmpl);
    close(f);
    errno = 0;
    r = lchmod(tmpl, 0640);
    stat(tmpl, &st);
    printf("lchmod(file, 0640): %d errno=%s mode %o\n", r, en(errno), st.st_mode & 0777);
    unlink(tmpl);

    /* §16 the user database through the name-service cache (D-16.10). */
    struct passwd *pw = getpwuid(0);
    struct passwd *pw2 = getpwuid(0);
    printf("getpwuid(0) twice: %s %s\n", pw ? "found" : "NULL", pw2 && strcmp(pw2->pw_name, "root") == 0 ? "root" : "?");
    return 0;
}
