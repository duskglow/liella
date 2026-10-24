/* Spec §21.1: setjmp/longjmp and the signal-mask forms. setjmp returns 0, then the value
 * longjmp passes (1 for 0); volatile locals survive; sigsetjmp(env, 1) restores the mask and
 * _setjmp/_longjmp leave it alone; siglongjmp out of a handler interrupting a blocking read
 * resumes with the handler's mask undone (F37); __longjmp_chk allows an upward jump. */
#define _GNU_SOURCE
#include <pthread.h>
#include <setjmp.h>
#include <signal.h>
#include <string.h>
#include <unistd.h>

extern void __longjmp_chk(jmp_buf env, int val) __attribute__((noreturn));

static jmp_buf env;
static sigjmp_buf senv;
static volatile int depth;

static void down(int n)
{
    depth = n;
    if (n == 0)
        longjmp(env, 0);
    down(n - 1);
}

static void on_alarm(int sig)
{
    (void)sig;
    siglongjmp(senv, 7);
}

/* Sends SIGALRM to the process after a while; blocks it itself, so the main thread takes it. */
static void *alarmer(void *arg)
{
    (void)arg;
    sigset_t s;
    sigemptyset(&s);
    sigaddset(&s, SIGALRM);
    pthread_sigmask(SIG_BLOCK, &s, NULL);
    usleep(50000);
    kill(getpid(), SIGALRM);
    return NULL;
}

static int blocked(int sig)
{
    sigset_t cur;
    sigprocmask(SIG_BLOCK, NULL, &cur);
    return sigismember(&cur, sig);
}

int main(void)
{
    volatile int count = 0;
    int r = setjmp(env);
    count++;
    if (r == 0) {
        if (count != 1)
            return 1;
        down(20);
    }
    if (r != 1 || count != 2 || depth != 0)
        return 2; /* longjmp(env, 0) makes setjmp return 1 */

    /* _setjmp/_longjmp: the mask is neither saved nor restored. */
    jmp_buf e2;
    if (_setjmp(e2) == 0) {
        sigset_t s;
        sigemptyset(&s);
        sigaddset(&s, SIGUSR1);
        sigprocmask(SIG_BLOCK, &s, NULL);
        _longjmp(e2, 5);
    }
    if (!blocked(SIGUSR1))
        return 3;

    /* sigsetjmp(env, 1): the mask at the time of the call comes back. */
    if (sigsetjmp(senv, 1) == 0) {
        sigset_t s;
        sigemptyset(&s);
        sigaddset(&s, SIGUSR2);
        sigprocmask(SIG_BLOCK, &s, NULL);
        siglongjmp(senv, 3);
    }
    if (blocked(SIGUSR2) || !blocked(SIGUSR1))
        return 4;

    /* Out of a handler that interrupted a blocking read (F37). */
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_alarm;
    sigaction(SIGALRM, &sa, NULL);
    int fds[2];
    if (pipe(fds) != 0)
        return 5;
    volatile int jumped = 0;
    int v = sigsetjmp(senv, 1);
    static pthread_t helper; /* set after sigsetjmp: must not be an automatic variable */
    if (v == 0) {
        if (pthread_create(&helper, NULL, alarmer, NULL) != 0)
            return 8;
        char c;
        (void)read(fds[0], &c, 1); /* blocks until the alarm */
        return 6;
    }
    jumped = v;
    if (jumped != 7 || blocked(SIGALRM))
        return 7; /* the handler's own mask (SIGALRM blocked) is undone */
    pthread_join(helper, NULL);

    /* __longjmp_chk to a frame above this one is allowed. */
    jmp_buf e3;
    if (setjmp(e3) == 0)
        __longjmp_chk(e3, 9);
    return 0;
}
