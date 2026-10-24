/* Spec §18.2: the alternative names __fork, __vfork, __clone and __register_atfork behave like
 * fork, vfork, clone and pthread_atfork (the handlers of __register_atfork run in order). */
#define _GNU_SOURCE
#include <sched.h>
#include <signal.h>
#include <stdlib.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

extern pid_t alias_fork(void) __asm__("__fork");
extern pid_t alias_vfork(void) __asm__("__vfork");
extern int alias_clone(int (*fn)(void *), void *stack, int flags, void *arg, ...) __asm__("__clone");
extern int alias_register_atfork(void (*prepare)(void), void (*parent)(void), void (*child)(void), void *dso) __asm__("__register_atfork");

static char trace[8];
static int ntrace;
static void prepare(void) { trace[ntrace++] = 'p'; }
static void parent(void) { trace[ntrace++] = 'P'; }
static void child(void) { trace[ntrace++] = 'C'; }

static int cloned(void *arg)
{
    return *(int *)arg;
}

static int status_of(pid_t pid)
{
    int st;
    /* waitpid is phase 11: the raw system call. */
    if (syscall(SYS_wait4, pid, &st, 0, NULL) != pid || !WIFEXITED(st))
        return -1;
    return WEXITSTATUS(st);
}

int main(void)
{
    if (alias_register_atfork(prepare, parent, child, NULL) != 0)
        return 1;
    pid_t p = alias_fork();
    if (p == 0)
        _exit(ntrace == 2 && trace[0] == 'p' && trace[1] == 'C' ? 7 : 1);
    if (p < 0 || status_of(p) != 7)
        return 2;
    if (ntrace != 2 || trace[0] != 'p' || trace[1] != 'P')
        return 3;

    /* __vfork: the parent resumes after the child exits; no atfork handlers run. */
    ntrace = 0;
    p = alias_vfork();
    if (p == 0)
        _exit(8);
    if (p < 0 || status_of(p) != 8 || ntrace != 0)
        return 4;

    /* __clone: a child on its own stack runs fn(arg) and exits with its result. */
    size_t sz = 64 * 1024;
    char *stack = malloc(sz);
    int code = 9;
    p = alias_clone(cloned, stack + sz, SIGCHLD, &code);
    if (p <= 0 || status_of(p) != 9)
        return 5;
    free(stack);
    return 0;
}
