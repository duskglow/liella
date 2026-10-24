/* Run a command with sync(2) intercepted (GATE-FSYNC-2): a seccomp filter hands every SYS_sync
 * the command issues to this process (SECCOMP_RET_USER_NOTIF), which counts it and answers it
 * with success (0) without running it, so the caller returns at once instead of waiting for the
 * whole machine's writeback. Every other system call runs normally. Needs no privilege
 * (PR_SET_NO_NEW_PRIVS) and a kernel with seccomp user notification (5.0).
 *
 * When the filter or its listener cannot be had (no seccomp in the kernel, a container that
 * refuses it), prints "SKIP: <reason>" on stderr and exits 77 without running the command.
 * Otherwise prints "sync_trap: trapped=N other=M" on stderr after the command ends (M counts
 * notifications for any other call: always 0 unless the filter is wrong) and exits with the
 * command's status (128 + the signal if it was killed).
 *
 * Usage: sync_trap COMMAND ARGS... */
#define _GNU_SOURCE
#include <errno.h>
#include <linux/audit.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <poll.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: sync_trap COMMAND ARGS...\n");
        return 2;
    }
    struct sock_filter filter[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, arch)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_sync, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_USER_NOTIF),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    struct sock_fprog prog = {(unsigned short)(sizeof filter / sizeof filter[0]), filter};
    struct seccomp_notif_sizes sizes;
    if (syscall(SYS_seccomp, SECCOMP_GET_NOTIF_SIZES, 0, &sizes) != 0) {
        fprintf(stderr, "SKIP: seccomp user notification unavailable (SECCOMP_GET_NOTIF_SIZES: %s)\n", strerror(errno));
        return 77;
    }
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0) {
        fprintf(stderr, "SKIP: PR_SET_NO_NEW_PRIVS refused (%s)\n", strerror(errno));
        return 77;
    }
    int listener = (int)syscall(SYS_seccomp, SECCOMP_SET_MODE_FILTER, SECCOMP_FILTER_FLAG_NEW_LISTENER, &prog);
    if (listener < 0) {
        fprintf(stderr, "SKIP: seccomp filter with a listener refused (%s)\n", strerror(errno));
        return 77;
    }
    size_t req_size = sizes.seccomp_notif > sizeof(struct seccomp_notif) ? sizes.seccomp_notif : sizeof(struct seccomp_notif);
    size_t resp_size = sizes.seccomp_notif_resp > sizeof(struct seccomp_notif_resp) ? sizes.seccomp_notif_resp : sizeof(struct seccomp_notif_resp);
    struct seccomp_notif *req = calloc(1, req_size);
    struct seccomp_notif_resp *resp = calloc(1, resp_size);
    if (req == NULL || resp == NULL) {
        perror("sync_trap: calloc");
        return 2;
    }
    pid_t pid = fork();
    if (pid < 0) {
        perror("sync_trap: fork");
        return 2;
    }
    if (pid == 0) {
        close(listener);
        execvp(argv[1], argv + 1);
        perror("sync_trap: exec");
        _exit(127);
    }
    /* A command that never ends is ended by the caller's timeout; this bounds a lost wrapper. */
    alarm(900);
    unsigned trapped = 0, other = 0;
    for (;;) {
        int status;
        pid_t w = waitpid(pid, &status, WNOHANG);
        if (w == pid) {
            fprintf(stderr, "sync_trap: trapped=%u other=%u\n", trapped, other);
            return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
        }
        if (w < 0 && errno != EINTR) {
            perror("sync_trap: waitpid");
            return 2;
        }
        struct pollfd p = {listener, POLLIN, 0};
        if (poll(&p, 1, 20) <= 0 || !(p.revents & POLLIN))
            continue;
        memset(req, 0, req_size);
        /* ENOENT: the caller died before its call was taken; the loop then sees it exit. */
        if (ioctl(listener, SECCOMP_IOCTL_NOTIF_RECV, req) != 0)
            continue;
        if (req->data.nr == __NR_sync)
            trapped++;
        else
            other++;
        memset(resp, 0, resp_size);
        resp->id = req->id;
        resp->val = 0;
        resp->error = 0;
        resp->flags = 0;
        (void)ioctl(listener, SECCOMP_IOCTL_NOTIF_SEND, resp);
    }
}
