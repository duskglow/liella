/* Spec §22.1: when the last thread exits, the process exits with status 0 (after atexit
 * handlers). The initial thread calls pthread_exit while another thread still runs. */
#include <pthread.h>
#include <stdlib.h>
#include <unistd.h>

static volatile int last_done;
static void at_exit(void) { _exit(last_done ? 0 : 3); }

static void *last(void *arg)
{
    (void)arg;
    usleep(30000);
    last_done = 1;
    return NULL;
}

int main(void)
{
    atexit(at_exit);
    pthread_t t;
    pthread_create(&t, NULL, last, NULL);
    pthread_exit(NULL);
    return 9;
}
