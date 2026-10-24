/* Run a command under a seccomp filter (D-11.8, D-11.11). With EPERM, ENOSYS or EACCES, io_uring
 * is refused, as a seccomp filter or `kernel.io_uring_disabled` refuses it on systems that
 * restrict it: `io_uring_setup` fails with the given error and every other system call is
 * allowed. With ALLOW, the filter allows every system call (it does not mention io_uring at all).
 * With PRCTL_EINVAL, `prctl(PR_GET_SECCOMP)` fails with EINVAL, with PRCTL_ZERO it returns 0
 * (`SECCOMP_RET_ERRNO` with no error: the call claims there is no filter), and with PRCTL_KILL
 * it kills the process; everything else is allowed.
 * The filter is inherited by every child and survives exec. Liella takes its worker path under
 * any filter (D-11.11). Needs no privilege (`PR_SET_NO_NEW_PRIVS`).
 *
 * Usage: aio_noring EPERM|ENOSYS|EACCES|ALLOW|PRCTL_EINVAL|PRCTL_ZERO|PRCTL_KILL COMMAND ARGS... */
#define _GNU_SOURCE
#include <errno.h>
#include <linux/audit.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    if (argc < 3) {
        fprintf(stderr, "usage: aio_noring EPERM|ENOSYS|EACCES|ALLOW|PRCTL_EINVAL|PRCTL_ZERO|PRCTL_KILL COMMAND ARGS...\n");
        return 2;
    }
    unsigned err = strcmp(argv[1], "ENOSYS") == 0 ? ENOSYS : strcmp(argv[1], "EACCES") == 0 ? EACCES : EPERM;
    struct sock_filter refuse[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, arch)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_io_uring_setup, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | (err & SECCOMP_RET_DATA)),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    struct sock_filter allow[] = {
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    /* What PR_GET_SECCOMP gets. */
    unsigned on_get = strcmp(argv[1], "PRCTL_ZERO") == 0 ? SECCOMP_RET_ERRNO
                      : strcmp(argv[1], "PRCTL_KILL") == 0 ? SECCOMP_RET_KILL_PROCESS
                                                           : SECCOMP_RET_ERRNO | EINVAL;
    struct sock_filter no_get[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_prctl, 0, 3),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, args[0])),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, PR_GET_SECCOMP, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, on_get),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    struct sock_fprog prog = {(unsigned short)(sizeof refuse / sizeof refuse[0]), refuse};
    if (strcmp(argv[1], "ALLOW") == 0)
        prog = (struct sock_fprog){(unsigned short)(sizeof allow / sizeof allow[0]), allow};
    else if (strncmp(argv[1], "PRCTL_", 6) == 0)
        prog = (struct sock_fprog){(unsigned short)(sizeof no_get / sizeof no_get[0]), no_get};
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 || prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &prog) != 0) {
        perror("aio_noring: seccomp");
        return 2;
    }
    execvp(argv[2], argv + 2);
    perror("aio_noring: exec");
    return 127;
}
