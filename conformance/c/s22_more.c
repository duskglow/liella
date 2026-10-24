/* Spec §22 clauses not covered elsewhere: a new thread inherits the creator's signal mask,
 * floating-point environment and affinity; attribute masks and affinity apply; a join cycle is
 * EDEADLK; an explicit real-time policy without privilege is EPERM; memory limits give EAGAIN;
 * pthread functions leave errno alone; more than PTHREAD_KEYS_MAX keys (D-22.5); static TLS at
 * one offset from the thread pointer in every thread; the rseq area is live; thread CPU clocks
 * advance; scheduling and affinity per thread; process-shared mutexes and semaphores across
 * fork; semaphore errors; close as a cancellation point still closes; jmp_buf pointers are
 * mangled; no shadow stack is enabled (D-21.3). */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <sched.h>
#include <semaphore.h>
#include <setjmp.h>
#include <signal.h>
#include <limits.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

extern ptrdiff_t __rseq_offset;
extern unsigned int __rseq_size;

struct seen {
    sigset_t mask;
    int round;
    cpu_set_t cpus;
    ptrdiff_t tls_off;
    int rseq_cpu;
};
static _Thread_local int tls_var = 1;

/* The SSE rounding mode (MXCSR bits 13-14): the floating-point environment without libm. */
static unsigned rounding(void)
{
    unsigned m;
    __asm__ volatile("stmxcsr %0" : "=m"(m));
    return (m >> 13) & 3;
}
static void set_rounding(unsigned r)
{
    unsigned m;
    __asm__ volatile("stmxcsr %0" : "=m"(m));
    m = (m & ~(3u << 13)) | (r << 13);
    __asm__ volatile("ldmxcsr %0" : : "m"(m));
}

static ptrdiff_t tls_offset(void)
{
    return (char *)&tls_var - (char *)__builtin_thread_pointer();
}

static void *observe(void *arg)
{
    struct seen *s = arg;
    pthread_sigmask(SIG_BLOCK, NULL, &s->mask);
    s->round = (int)rounding();
    sched_getaffinity(0, sizeof s->cpus, &s->cpus);
    s->tls_off = tls_offset();
    if (__rseq_size != 0)
        s->rseq_cpu = (int)*(volatile uint32_t *)((char *)__builtin_thread_pointer() + __rseq_offset + 4);
    else
        s->rseq_cpu = sched_getcpu();
    return NULL;
}

static pthread_t peer;
static volatile int peer_ready;
static volatile pid_t peer_tid;
static void *join_peer(void *arg)
{
    (void)arg;
    peer_tid = gettid();
    peer_ready = 1;
    while (peer_ready != 2)
        usleep(1000);
    return (void *)(intptr_t)pthread_join(pthread_self() == peer ? peer : peer, NULL);
}

static void *nothing(void *arg) { return arg; }

static void *spin_cpu(void *arg)
{
    volatile unsigned long n = 0;
    while (!*(volatile int *)arg)
        n++;
    return NULL;
}

static int closer_fd;
static void *closer(void *arg)
{
    (void)arg;
    pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
    while (*(volatile int *)&closer_fd >= 0 && closer_fd != -2)
        usleep(1000);
    return NULL;
}
static int pending_fd;
static void *close_when_pending(void *arg)
{
    (void)arg;
    pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
    while (!*(volatile int *)&closer_fd)
        usleep(1000);
    pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, NULL);
    close(pending_fd); /* a cancellation point with a request pending */
    return (void *)1;
}

static sem_t post_sem;
static void post_from_handler(int s) { (void)s; sem_post(&post_sem); }

/* Watchdog: a step that never returns fails within a minute and names itself (exit 99) instead
 * of hanging until the harness's 600 s limit (FLAKE-PTHREAD-CANCEL). */
static const char *volatile step = "start";
static void stalled(int s)
{
    (void)s;
    static const char head[] = "s22_more: watchdog: stalled in step ";
    write(2, head, sizeof head - 1);
    write(2, step, strlen(step));
    write(2, "\n", 1);
    _exit(99);
}

/* Waits (at most 20 s) until the thread whose /proc ".../syscall" file is `path` is blocked in a
 * system call other than a sleep (nanosleep 35, clock_nanosleep 230): a fixed sleep before the
 * other side acts lost the race on a loaded host. After the cap the caller goes on (the check
 * then decides). The file must be readable: it needs ptrace access, which Yama (ptrace_scope 1)
 * denies to a child reading its parent, so only threads of this process are polled. An unreadable
 * file fails at once (exit 98, naming the step) instead of silently costing the full 20 s cap
 * (CI-SLOW). */
static void wait_blocked(const char *path)
{
    for (int i = 0; i < 2000; i++) {
        char b[32];
        int fd = open(path, O_RDONLY | O_CLOEXEC);
        ssize_t n = fd >= 0 ? read(fd, b, sizeof b - 1) : -1;
        if (fd >= 0)
            close(fd);
        if (n < 0) {
            static const char head[] = "s22_more: cannot read /proc task syscall file in step ";
            write(2, head, sizeof head - 1);
            write(2, step, strlen(step));
            write(2, "\n", 1);
            _exit(98);
        }
        if (n > 0 && b[0] >= '0' && b[0] <= '9') {
            long nr = 0;
            for (ssize_t j = 0; j < n && b[j] >= '0' && b[j] <= '9'; j++)
                nr = nr * 10 + (b[j] - '0');
            if (nr != 35 && nr != 230)
                return;
        }
        usleep(10000);
    }
}

static void task_syscall_path(char *out, size_t size, pid_t pid, pid_t tid)
{
    snprintf(out, size, "/proc/%d/task/%d/syscall", (int)pid, (int)tid);
}

static volatile int got_usr1;
static void on_usr1(int s) { (void)s; got_usr1 = 1; }

/* Waits until the main thread (tid == pid) is blocked, then sends it SIGUSR1 directly (tgkill: a
 * process-directed signal could be taken by this thread instead). */
static void *interrupt_main(void *arg)
{
    const char *path = arg;
    wait_blocked(path); /* main is in sem_wait: a signal before it would leave it waiting */
    syscall(SYS_tgkill, (int)getpid(), (int)getpid(), SIGUSR1);
    return NULL;
}

int main(void)
{
    signal(SIGALRM, stalled);
    alarm(60);
    /* Inheritance: mask, rounding mode, affinity. */
    sigset_t m;
    sigemptyset(&m);
    sigaddset(&m, SIGUSR2);
    pthread_sigmask(SIG_BLOCK, &m, NULL);
    set_rounding(2); /* upward */
    cpu_set_t one;
    CPU_ZERO(&one);
    CPU_SET(0, &one);
    sched_setaffinity(0, sizeof one, &one);
    struct seen s;
    pthread_t t;
    pthread_create(&t, NULL, observe, &s);
    pthread_join(t, NULL);
    if (!sigismember(&s.mask, SIGUSR2) || s.round != 2 || CPU_COUNT(&s.cpus) != 1 || !CPU_ISSET(0, &s.cpus))
        return 1;
    if (s.tls_off != tls_offset())
        return 2; /* static TLS at one offset from the thread pointer */
    if (s.rseq_cpu != 0)
        return 3; /* the rseq area reports the CPU it runs on */
    set_rounding(0);
    cpu_set_t all;
    CPU_ZERO(&all);
    for (int i = 0; i < CPU_SETSIZE && i < 64; i++)
        CPU_SET(i, &all);
    sched_setaffinity(0, sizeof all, &all);

    /* Attribute mask and affinity. */
    pthread_attr_t a;
    pthread_attr_init(&a);
    sigset_t want;
    sigemptyset(&want);
    sigaddset(&want, SIGHUP);
    if (pthread_attr_setsigmask_np(&a, &want) != 0)
        return 4;
    cpu_set_t c1;
    CPU_ZERO(&c1);
    CPU_SET(1, &c1);
    if (pthread_attr_setaffinity_np(&a, sizeof c1, &c1) != 0)
        return 5;
    pthread_create(&t, &a, observe, &s);
    pthread_join(t, NULL);
    pthread_attr_destroy(&a);
    if (!sigismember(&s.mask, SIGHUP) || sigismember(&s.mask, SIGUSR2) || CPU_COUNT(&s.cpus) != 1 || !CPU_ISSET(1, &s.cpus))
        return 6;
    pthread_sigmask(SIG_UNBLOCK, &m, NULL);

    /* EPERM: an explicit real-time policy without privilege. */
    pthread_attr_init(&a);
    pthread_attr_setinheritsched(&a, PTHREAD_EXPLICIT_SCHED);
    pthread_attr_setschedpolicy(&a, SCHED_FIFO);
    struct sched_param sp = {.sched_priority = 10};
    pthread_attr_setschedparam(&a, &sp);
    int e = pthread_create(&t, &a, nothing, NULL);
    if (getuid() != 0 && e != EPERM)
        return 7;
    if (e == 0)
        pthread_join(t, NULL);
    pthread_attr_destroy(&a);

    /* EAGAIN under a small address-space limit. */
    struct rlimit old, lim;
    getrlimit(RLIMIT_AS, &old);
    lim = old;
    lim.rlim_cur = 64 << 20;
    setrlimit(RLIMIT_AS, &lim);
    pthread_attr_init(&a);
    pthread_attr_setstacksize(&a, 512 << 20);
    errno = 0;
    e = pthread_create(&t, &a, nothing, NULL);
    pthread_attr_destroy(&a);
    setrlimit(RLIMIT_AS, &old);
    if (e != EAGAIN)
        return 8;
    if (errno != 0)
        return 9; /* error numbers, errno untouched */

    /* A join cycle. */
    peer = pthread_self();
    pthread_create(&t, NULL, join_peer, NULL);
    while (!peer_ready)
        usleep(1000);
    /* t joins main (it waits); main joining t now would be a cycle. */
    step = "join cycle";
    char path[64];
    task_syscall_path(path, sizeof path, getpid(), peer_tid);
    peer_ready = 2;
    wait_blocked(path); /* t is in its join of main */
    if (pthread_join(t, NULL) != EDEADLK)
        return 10;
    /* Release t: it is still waiting for main; detach it and let it be. */
    pthread_detach(t);

    /* More keys than PTHREAD_KEYS_MAX. */
    pthread_key_t k;
    int made = 0;
    while (pthread_key_create(&k, NULL) == 0 && made < 2000)
        made++;
    if (made < 2000)
        return 11;

    /* Thread CPU clock. */
    volatile int stop = 0;
    pthread_create(&t, NULL, spin_cpu, (void *)&stop);
    clockid_t cid;
    if (pthread_getcpuclockid(t, &cid) != 0)
        return 12;
    struct timespec c0, c1t;
    clock_gettime(cid, &c0);
    /* The spinner needs CPU time to advance its clock: on a loaded machine it may not be scheduled within a fixed
     * wall-clock sleep, so wait (up to 20 s) for the clock to move. A clock that never moves still fails. */
    int moved = 0;
    for (int i = 0; i < 2000 && !moved; i++) {
        usleep(10000);
        clock_gettime(cid, &c1t);
        moved = c1t.tv_sec * 1000000000L + c1t.tv_nsec > c0.tv_sec * 1000000000L + c0.tv_nsec;
    }
    if (!moved)
        return 13;

    /* Per-thread scheduling and affinity. */
    struct sched_param z = {.sched_priority = 0};
    if (pthread_setschedparam(t, SCHED_BATCH, &z) != 0)
        return 14;
    int pol;
    struct sched_param got;
    if (pthread_getschedparam(t, &pol, &got) != 0 || pol != SCHED_BATCH)
        return 15;
    if (pthread_setaffinity_np(t, sizeof one, &one) != 0)
        return 16;
    cpu_set_t back;
    if (pthread_getaffinity_np(t, sizeof back, &back) != 0 || CPU_COUNT(&back) != 1)
        return 17;
    stop = 1;
    pthread_join(t, NULL);

    /* Process-shared mutex and semaphore across fork. */
    struct shm {
        pthread_mutex_t mu;
        sem_t sem;
        long counter;
    } *sh = mmap(NULL, sizeof *sh, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    pthread_mutexattr_t ma;
    pthread_mutexattr_init(&ma);
    pthread_mutexattr_setpshared(&ma, PTHREAD_PROCESS_SHARED);
    pthread_mutex_init(&sh->mu, &ma);
    sem_init(&sh->sem, 1, 0);
    pid_t pid = fork();
    for (int i = 0; i < 20000; i++) {
        pthread_mutex_lock(&sh->mu);
        sh->counter++;
        pthread_mutex_unlock(&sh->mu);
    }
    if (pid == 0) {
        sem_post(&sh->sem);
        _exit(0);
    }
    sem_wait(&sh->sem);
    syscall(SYS_wait4, pid, NULL, 0, NULL);
    if (sh->counter != 40000)
        return 18;

    /* Semaphore errors. */
    sem_t sm;
    if (sem_init(&sm, 0, (unsigned)SEM_VALUE_MAX + 1u) != -1 || errno != EINVAL)
        return 19;
    sem_init(&sm, 0, SEM_VALUE_MAX);
    if (sem_post(&sm) != -1 || errno != EOVERFLOW)
        return 20;
    sem_init(&sm, 0, 0);
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_usr1;
    sigaction(SIGUSR1, &sa, NULL);
    pid = getpid();
    pthread_create(&t, NULL, nothing, NULL);
    pthread_join(t, NULL);
    step = "sem_wait interrupted by a signal";
    task_syscall_path(path, sizeof path, pid, pid);
    pthread_create(&t, NULL, interrupt_main, path);
    if (sem_wait(&sm) != -1 || errno != EINTR || !got_usr1)
        return 21;
    pthread_join(t, NULL);

    /* close with a cancellation request pending: closed, and the thread cancelled. */
    int fds[2];
    pipe(fds);
    pending_fd = fds[0];
    closer_fd = 0;
    pthread_create(&t, NULL, close_when_pending, NULL);
    usleep(10000);
    pthread_cancel(t);
    closer_fd = 1;
    void *r;
    pthread_join(t, &r);
    if (r != PTHREAD_CANCELED || fcntl(pending_fd, F_GETFD) != -1 || errno != EBADF)
        return 22;

    /* jmp_buf: stack and return addresses are mangled. */
    jmp_buf jb;
    if (setjmp(jb) == 0) {
        uintptr_t sp_slot = (uintptr_t)jb[0].__jmpbuf[6];
        uintptr_t here = (uintptr_t)__builtin_frame_address(0);
        if (sp_slot > here - 4096 && sp_slot < here + 4096)
            return 23;
    }
    /* Joining a detached thread is refused (EINVAL; undefined behaviour by the contract). */
    stop = 0;
    pthread_attr_init(&a);
    pthread_attr_setdetachstate(&a, PTHREAD_CREATE_DETACHED);
    pthread_create(&t, &a, spin_cpu, (void *)&stop);
    pthread_attr_destroy(&a);
    if (pthread_join(t, NULL) != EINVAL)
        return 24;
    stop = 1;

    /* The default stack: the RLIMIT_STACK soft limit at startup (2 MiB if unlimited). */
    struct rlimit rs;
    getrlimit(RLIMIT_STACK, &rs);
    pthread_attr_t d;
    size_t dsz;
    if (pthread_getattr_default_np(&d) != 0 || pthread_attr_getstacksize(&d, &dsz) != 0)
        return 25;
    if (rs.rlim_cur == RLIM_INFINITY ? dsz != 2u << 20 : dsz != (size_t)rs.rlim_cur)
        return 26;

    /* Priority protection: the ceiling round-trips. */
    pthread_mutexattr_t pa;
    pthread_mutexattr_init(&pa);
    if (pthread_mutexattr_setprotocol(&pa, PTHREAD_PRIO_PROTECT) != 0 || pthread_mutexattr_setprioceiling(&pa, 10) != 0)
        return 27;
    int pc;
    if (pthread_mutexattr_getprioceiling(&pa, &pc) != 0 || pc != 10)
        return 28;
    pthread_mutex_t pm;
    pthread_mutex_init(&pm, &pa);
    int oldc;
    if (pthread_mutex_getprioceiling(&pm, &pc) != 0 || pc != 10 || pthread_mutex_setprioceiling(&pm, 20, &oldc) != 0 || oldc != 10)
        return 29;
    if (pthread_mutex_lock(&pm) != 0 || pthread_mutex_unlock(&pm) != 0)
        return 30;

    /* sem_post from a signal handler (async-signal-safe). */
    sem_init(&post_sem, 0, 0);
    struct sigaction pa2;
    memset(&pa2, 0, sizeof pa2);
    pa2.sa_handler = post_from_handler;
    sigaction(SIGUSR2, &pa2, NULL);
    raise(SIGUSR2);
    if (sem_trywait(&post_sem) != 0)
        return 31;
    (void)closer;
    return 0;
}
