/* fork + _exit + waitpid (TIMING-COV-FORK-COW-FAULTS): regen.sh traces this probe into the child
 * (HOT_TEXT_FOLLOW_FORK=child), so the list holds the fork child's path, which faults on every
 * text window it touches (fork copies no page tables of file-backed text). The first waitpid finds
 * no child (ECHILD): it runs the wait path before the fork, where the trace still sees it. */
#include <sys/wait.h>
#include <unistd.h>

int main(void)
{
    (void)waitpid(-1, NULL, WNOHANG);
    pid_t p = fork();
    if (p == 0)
        _exit(0);
    return p > 0 && waitpid(p, NULL, 0) == p ? 0 : 1;
}
