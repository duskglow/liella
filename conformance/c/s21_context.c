/* Spec §21.2: user contexts. getcontext/setcontext resume; makecontext passes its arguments
 * (more than six: the rest go on the new stack) and returns to uc_link; swapcontext switches
 * back and forth and switches the signal mask; a context function whose uc_link is NULL ends
 * the thread (here the last one: the process exits with status 0, after atexit handlers). */
#define _GNU_SOURCE
#include <signal.h>
#include <stdlib.h>
#include <ucontext.h>
#include <unistd.h>

static ucontext_t main_uc, co_uc, end_uc;
static char co_stack[64 * 1024], end_stack[64 * 1024];
static long sum;
static int pings;
static int finished;

static void co(int a, int b, int c, int d, int e, int f, int g, int h)
{
    sum = a + 2L * b + 3L * c + 4L * d + 5L * e + 6L * f + 7L * g + 8L * h;
    for (int i = 0; i < 3; i++) {
        pings++;
        swapcontext(&co_uc, &main_uc);
    }
}

static void last(void)
{
    finished = 1;
}

static void at_exit(void)
{
    _exit(finished == 1 ? 0 : 50);
}

static int blocked(int sig)
{
    sigset_t cur;
    sigprocmask(SIG_BLOCK, NULL, &cur);
    return sigismember(&cur, sig);
}

int main(void)
{
    volatile int resumed = 0;
    ucontext_t uc;
    if (getcontext(&uc) != 0)
        return 1;
    if (resumed++ == 0)
        setcontext(&uc);
    if (resumed != 2)
        return 2;

    if (getcontext(&co_uc) != 0)
        return 3;
    co_uc.uc_stack.ss_sp = co_stack;
    co_uc.uc_stack.ss_size = sizeof co_stack;
    co_uc.uc_link = &main_uc;
    sigemptyset(&co_uc.uc_sigmask);
    sigaddset(&co_uc.uc_sigmask, SIGUSR1); /* the coroutine runs with SIGUSR1 blocked */
    makecontext(&co_uc, (void (*)(void))co, 8, 1, 2, 3, 4, 5, 6, 7, 8);
    for (int i = 1; i <= 3; i++) {
        if (swapcontext(&main_uc, &co_uc) != 0)
            return 4;
        if (pings != i || blocked(SIGUSR1))
            return 5; /* back in main with main's mask */
    }
    if (sum != 1 + 4 + 9 + 16 + 25 + 36 + 49 + 64)
        return 6;
    if (swapcontext(&main_uc, &co_uc) != 0)
        return 7; /* co returns: uc_link resumes main */

    /* uc_link NULL: the thread ends when the function returns. */
    atexit(at_exit);
    if (getcontext(&end_uc) != 0)
        return 8;
    end_uc.uc_stack.ss_sp = end_stack;
    end_uc.uc_stack.ss_size = sizeof end_stack;
    end_uc.uc_link = NULL;
    makecontext(&end_uc, last, 0);
    setcontext(&end_uc);
    return 9;
}
