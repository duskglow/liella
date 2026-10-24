/* Spec §18.4 (D-18.3): posix_spawn and posix_spawnp, whose child shares the caller's memory
 * (CLONE_VM | CLONE_VFORK) and must therefore touch nothing the caller relies on. Checked: the
 * exact error number of every way a spawn fails (exec, search, file actions), no child left
 * behind, the exit status of one that runs, the caller's signal mask and errno after the call,
 * and, in a multi-threaded caller under load, (a) the heap stays intact and the spawns finish
 * (a child that took an allocator lock another thread holds would never end), and (b) signals sent
 * to the whole process group while spawning never kill a child or run the caller's handler in
 * it. Failures are printed and counted; the exit status is the count. */
#define _GNU_SOURCE
#include <errno.h>
#include <malloc.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern char **environ;

static int fails;
#define CHECK(c, what)                                                         \
    do {                                                                       \
        if (!(c)) {                                                            \
            fprintf(stderr, "FAIL line %d: %s\n", __LINE__, what);             \
            __atomic_fetch_add(&fails, 1, __ATOMIC_RELAXED);                   \
        }                                                                      \
    } while (0)

static int wait_status(pid_t pid)
{
    int st = -1;
    while (waitpid(pid, &st, 0) == -1 && errno == EINTR) {
    }
    return st;
}

static int no_children(void)
{
    errno = 0;
    return waitpid(-1, NULL, WNOHANG) == -1 && errno == ECHILD;
}

static void failing_spawns(void)
{
    char *argv[] = {"x", NULL};
    pid_t pid = 12345;
    errno = 0;
    CHECK(posix_spawn(&pid, "/nonexistent/prog", NULL, NULL, argv, environ) == ENOENT, "missing program: ENOENT");
    CHECK(posix_spawnp(&pid, "no-such-program-xyz", NULL, NULL, argv, environ) == ENOENT, "missing program on PATH: ENOENT");
    CHECK(posix_spawn(&pid, "/etc/passwd", NULL, NULL, argv, environ) == EACCES, "data file: EACCES");
    CHECK(posix_spawn(&pid, "/", NULL, NULL, argv, environ) == EACCES, "directory: EACCES");

    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addopen(&fa, 5, "/nonexistent/file", O_RDONLY, 0);
    int rc0 = posix_spawn(&pid, "/bin/true", &fa, NULL, argv, environ);
    CHECK(rc0 == ENOENT, "failing open action: ENOENT");
    posix_spawn_file_actions_destroy(&fa);

    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_adddup2(&fa, 999, 5);
    CHECK(posix_spawn(&pid, "/bin/true", &fa, NULL, argv, environ) == EBADF, "failing dup2 action: EBADF");
    posix_spawn_file_actions_destroy(&fa);

    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addchdir_np(&fa, "/nonexistent/dir");
    CHECK(posix_spawn(&pid, "/bin/true", &fa, NULL, argv, environ) == ENOENT, "failing chdir action: ENOENT");
    posix_spawn_file_actions_destroy(&fa);
    CHECK(no_children(), "no child left behind by the failed spawns");
}

static void running_spawns(void)
{
    char *argv_true[] = {"true", NULL};
    char *argv_seven[] = {"sh", "-c", "exit 7", NULL};
    pid_t pid = 0;
    sigset_t before, after;
    pthread_sigmask(SIG_SETMASK, NULL, &before);
    errno = 4242;
    CHECK(posix_spawn(&pid, "/bin/true", NULL, NULL, argv_true, environ) == 0 && pid > 0, "spawn /bin/true");
    CHECK(errno == 4242, "errno untouched by a successful spawn");
    int st = wait_status(pid);
    CHECK(WIFEXITED(st) && WEXITSTATUS(st) == 0, "true exits 0");
    CHECK(posix_spawnp(&pid, "sh", NULL, NULL, argv_seven, environ) == 0, "spawnp sh");
    st = wait_status(pid);
    CHECK(WIFEXITED(st) && WEXITSTATUS(st) == 7, "the child's own status is kept");
    pthread_sigmask(SIG_SETMASK, NULL, &after);
    CHECK(memcmp(&before, &after, sizeof before) == 0, "the caller's signal mask is as it was");
    CHECK(no_children(), "no child left behind");

    /* The child shares the heap: whatever it allocated would stay in the caller's. */
    size_t used = mallinfo2().uordblks;
    for (int i = 0; i < 50; i++) {
        CHECK(posix_spawn(&pid, "/bin/true", NULL, NULL, argv_true, environ) == 0, "spawn in the leak loop");
        wait_status(pid);
    }
    CHECK(mallinfo2().uordblks == used, "the heap in use is unchanged by 50 spawns (the child allocates nothing)");
}

/* ---- load: allocator threads and group signals while spawning ---- */

static volatile int stop;
static pid_t parent_pid;
static volatile int handler_calls, handler_in_wrong_process;

static void on_usr1(int sig)
{
    (void)sig;
    __atomic_fetch_add(&handler_calls, 1, __ATOMIC_RELAXED);
    if (getpid() != parent_pid)
        __atomic_store_n(&handler_in_wrong_process, 1, __ATOMIC_RELAXED);
}

static void *hammer(void *arg)
{
    unsigned seed = (unsigned)(size_t)arg * 2654435761u + 1;
    unsigned char *slot[64] = {0};
    size_t len[64] = {0};
    while (!__atomic_load_n(&stop, __ATOMIC_RELAXED)) {
        seed = seed * 1103515245u + 12345u;
        unsigned i = (seed >> 8) & 63;
        if (slot[i]) {
            for (size_t k = 0; k < len[i]; k++)
                if (slot[i][k] != (unsigned char)(i + 1)) {
                    CHECK(0, "a heap block was corrupted");
                    return NULL;
                }
            free(slot[i]);
            slot[i] = NULL;
        } else {
            len[i] = 16 + (seed >> 16) % 4000;
            slot[i] = malloc(len[i]);
            if (!slot[i]) {
                CHECK(0, "malloc failed");
                return NULL;
            }
            memset(slot[i], (int)(i + 1), len[i]);
        }
    }
    for (int i = 0; i < 64; i++)
        free(slot[i]);
    return NULL;
}

static void *shooter(void *arg)
{
    (void)arg;
    while (!__atomic_load_n(&stop, __ATOMIC_RELAXED)) {
        kill(0, SIGUSR1);
        struct timespec ts = {0, 20000};
        nanosleep(&ts, NULL);
    }
    return NULL;
}

static void under_load(void)
{
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_usr1;
    sigaction(SIGUSR1, &sa, NULL);
    pthread_t h[4], s;
    for (int i = 0; i < 4; i++)
        pthread_create(&h[i], NULL, hammer, (void *)(size_t)(i + 1));
    pthread_create(&s, NULL, shooter, NULL);

    /* The program runs with SIGUSR1 blocked, so a group signal after the exec cannot kill it:
     * any child that dies of one was exposed before that. */
    posix_spawnattr_t at;
    sigset_t m;
    sigemptyset(&m);
    sigaddset(&m, SIGUSR1);
    posix_spawnattr_init(&at);
    posix_spawnattr_setsigmask(&at, &m);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSIGMASK);
    char *argv[] = {"true", NULL};
    int bad = 0;
    for (int i = 0; i < 400; i++) {
        pid_t pid;
        int rc = (i & 1) ? posix_spawn(&pid, "/bin/true", NULL, &at, argv, environ) : posix_spawnp(&pid, "true", NULL, &at, argv, environ);
        if (rc != 0) {
            bad++;
            continue;
        }
        int st = wait_status(pid);
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
            bad++;
        }
    }
    __atomic_store_n(&stop, 1, __ATOMIC_RELAXED);
    pthread_join(s, NULL);
    for (int i = 0; i < 4; i++)
        pthread_join(h[i], NULL);
    CHECK(bad == 0, "every spawn under load ran its program to a clean exit");
    CHECK(!handler_in_wrong_process, "the caller's handler never ran in a child");
    fprintf(stderr, "info: %d handler calls in the caller\n", handler_calls);
    posix_spawnattr_destroy(&at);
}

int main(void)
{
    parent_pid = getpid();
    setpgid(0, 0); /* the group signals below reach only this program and its children */
    failing_spawns();
    running_spawns();
    under_load();
    return fails;
}
