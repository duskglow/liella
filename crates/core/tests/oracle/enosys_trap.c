/* Run a command with chosen system calls refused with ENOSYS, as a kernel that predates them
 * (or a seccomp policy that does not know them) refuses them; every other call is allowed
 * (tests/enosys_fallbacks.rs). The filter is inherited by every child and survives exec.
 * Needs no privilege (`PR_SET_NO_NEW_PRIVS`), like oracle/aio_noring.c.
 *
 * Usage: enosys_trap NAME[,NAME...] COMMAND ARGS...
 * NAME: clone3 close_range faccessat2 fchmodat2 getrandom openat2 pidfd_open statx */
#define _GNU_SOURCE
#include <errno.h>
#include <linux/audit.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <sys/prctl.h>
#include <unistd.h>

/* x86_64 numbers; the headers of older systems lack the newest ones. */
static const struct {
    const char *name;
    unsigned nr;
} calls[] = {
    {"getrandom", 318}, {"statx", 332},       {"pidfd_open", 434}, {"clone3", 435},
    {"close_range", 436}, {"openat2", 437}, {"faccessat2", 439}, {"fchmodat2", 452},
};

int main(int argc, char **argv)
{
    if (argc < 3) {
        fprintf(stderr, "usage: enosys_trap NAME[,NAME...] COMMAND ARGS...\n");
        return 2;
    }
    unsigned nrs[16];
    unsigned n = 0;
    char list[256];
    snprintf(list, sizeof list, "%s", argv[1]);
    for (char *save = NULL, *name = strtok_r(list, ",", &save); name; name = strtok_r(NULL, ",", &save)) {
        unsigned i = 0;
        while (i < sizeof calls / sizeof calls[0] && strcmp(calls[i].name, name) != 0)
            i++;
        if (i == sizeof calls / sizeof calls[0] || n == 16) {
            fprintf(stderr, "enosys_trap: unknown or too many system calls: %s\n", name);
            return 2;
        }
        nrs[n++] = calls[i].nr;
    }
    /* Other architectures pass; on x86_64 each listed number returns ENOSYS. */
    struct sock_filter f[4 + 16 + 2];
    unsigned k = 0;
    f[k++] = (struct sock_filter)BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, arch));
    f[k++] = (struct sock_filter)BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0);
    f[k++] = (struct sock_filter)BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW);
    f[k++] = (struct sock_filter)BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr));
    for (unsigned i = 0; i < n; i++) {
        /* A match jumps over the remaining tests and the ALLOW to the ERRNO return. */
        unsigned char skip = (unsigned char)(n - 1 - i + 1);
        f[k++] = (struct sock_filter)BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, nrs[i], skip, 0);
    }
    f[k++] = (struct sock_filter)BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW);
    f[k++] = (struct sock_filter)BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | (ENOSYS & SECCOMP_RET_DATA));
    struct sock_fprog prog = {(unsigned short)k, f};
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 || prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &prog) != 0) {
        perror("enosys_trap: seccomp");
        return 2;
    }
    execvp(argv[2], argv + 2);
    perror("enosys_trap: exec");
    return 127;
}
