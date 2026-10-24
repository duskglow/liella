/* Spec §25.2: semctl through real C varargs: the fourth argument (a union semun) arrives when
 * the command takes one and is ignored when it does not. Each failing CHECK exits with its
 * line number. */
#define _GNU_SOURCE
#include <errno.h>
#include <sys/ipc.h>
#include <sys/sem.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

union semun {
    int val;
    struct semid_ds *buf;
    unsigned short *array;
};

int main(void)
{
    int id = semget(IPC_PRIVATE, 2, 0600);
    CHECK("sym_semget", id >= 0);
    union semun arg;
    arg.val = 9;
    CHECK("s25_2_c", semctl(id, 1, SETVAL, arg) == 0);
    CHECK("s25_2_c", semctl(id, 1, GETVAL) == 9);
    unsigned short values[2] = { 3, 4 };
    arg.array = values;
    CHECK("s25_2_c", semctl(id, 0, SETALL, arg) == 0);
    unsigned short got[2] = { 0, 0 };
    arg.array = got;
    CHECK("s25_2_c", semctl(id, 0, GETALL, arg) == 0 && got[0] == 3 && got[1] == 4);
    struct semid_ds ds;
    arg.buf = &ds;
    CHECK("s25_2_c", semctl(id, 0, IPC_STAT, arg) == 0 && ds.sem_nsems == 2);
    errno = 0;
    CHECK("s25_2_f", semctl(id, 0, 0x7777) == -1 && errno == EINVAL);
    CHECK("s25_2_c", semctl(id, 0, IPC_RMID) == 0);
    return 0;
}
