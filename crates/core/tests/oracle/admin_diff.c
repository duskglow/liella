/* Library identification and the system-administration thin wrappers (spec §24.5-24.6;
 * milestone 5, 2026-10-04): built against the host C library and statically against Liella's
 * products, the outputs must match line for line (admin_products.rs), except the recorded
 * differences. Black-box: every line is a result of a public function.
 *
 * Usage: admin_diff MODE, where MODE is
 *   plain  - as an unprivileged user: the identification strings, and every wrapper's answer
 *            where the kernel refuses (privilege) or checks an argument;
 *   userns - in a private user namespace (the harness runs `unshare -U --map-root-user`): the
 *            capability sets of a namespace root, raised with capset; the wrappers whose
 *            privilege the namespace does not grant still refuse;
 *   pidns  - as the first process of a private PID namespace in a private user namespace
 *            (`unshare -U -p -f --map-root-user`): reboot's commands reach the kernel, which
 *            refuses the ones a PID namespace has no meaning for and ends the namespace for
 *            RB_POWER_OFF (the harness reports how the namespace ended).
 * Nothing here can change the system: the program refuses to run the namespace modes unless it
 * is in a user namespace (its uid_map is not the identity map). */
#define _GNU_SOURCE
#include <errno.h>
#include <gnu/libc-version.h>
#include <linux/capability.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/io.h>
#include <sys/klog.h>
#include <sys/quota.h>
#include <sys/reboot.h>
#include <unistd.h>

/* Exported but declared in no installed header (spec §24.6). */
extern int capget(cap_user_header_t, cap_user_data_t);
extern int capset(cap_user_header_t, const cap_user_data_t);
extern int init_module(void *, unsigned long, const char *);
extern int delete_module(const char *, unsigned int);
extern int modify_ldt(int, void *, unsigned long);

/* Each step names itself on stderr before it runs: when a run is cut off by its watchdog, the
   harness reports the step that never returned (stdout and stderr are unbuffered). */
#define R(label, call) do { fprintf(stderr, "step: %s\n", label); errno = 0; long r_ = (long)(call); int e_ = errno; printf("%s: %ld errno=%d\n", label, r_, e_); } while (0)

static void version(void)
{
    printf("== identification\n");
    const char *v = gnu_get_libc_version(), *r = gnu_get_libc_release();
    printf("version [%s] release [%s] same=%d\n", v, r, v == gnu_get_libc_version() && r == gnu_get_libc_release());
    char cs[64];
    confstr(_CS_GNU_LIBC_VERSION, cs, sizeof cs);
    printf("confstr [%s] agrees=%d\n", cs, strncmp(cs, "glibc ", 6) == 0 && strcmp(cs + 6, v) == 0);
}

static void show_caps(const char *what, const struct __user_cap_data_struct *d)
{
    printf("  %s eff %08x %08x perm %08x %08x inh %08x %08x\n", what, d[0].effective, d[1].effective,
           d[0].permitted, d[1].permitted, d[0].inheritable, d[1].inheritable);
}

static void capabilities(int raise)
{
    printf("== capget / capset\n");
    struct __user_cap_header_struct hdr = {0, 0};
    struct __user_cap_data_struct data[2];
    memset(data, 0xAA, sizeof data);
    R("capget version 0", capget(&hdr, data));
    printf("  version now %#x data %s\n", hdr.version, data[0].effective == 0xAAAAAAAA ? "untouched" : "written");
    hdr.version = _LINUX_CAPABILITY_VERSION_3;
    R("capget v3", capget(&hdr, data));
    show_caps("now", data);
    R("capget v3 NULL data", capget(&hdr, NULL));
    hdr.pid = -5;
    R("capget pid -5", capget(&hdr, data));
    hdr.pid = 0x7ffffff0;
    R("capget pid absent", capget(&hdr, data));
    hdr.pid = 0;
    R("capget NULL header", capget(NULL, data));
    R("capset unchanged", capset(&hdr, data));
    struct __user_cap_data_struct more[2];
    memcpy(more, data, sizeof more);
    more[0].inheritable = more[0].permitted;
    more[1].inheritable = more[1].permitted;
    R("capset inheritable = permitted", capset(&hdr, more));
    memcpy(more, data, sizeof more);
    more[0].effective = more[0].permitted = 0xffffffff;
    R("capset all effective and permitted bits", capset(&hdr, more));
    hdr.version = 0x12345678;
    R("capset unknown version", capset(&hdr, data));
    printf("  version now %#x\n", hdr.version);
    hdr.version = _LINUX_CAPABILITY_VERSION_3;
    R("capget again", capget(&hdr, data));
    show_caps("after", data);
    if (raise) {
        /* A namespace root drops CAP_SYS_ADMIN (21) from its effective set and takes it back. */
        memcpy(more, data, sizeof more);
        more[0].effective &= ~(1u << 21);
        R("capset drop CAP_SYS_ADMIN", capset(&hdr, more));
        R("capget", capget(&hdr, data));
        show_caps("dropped", data);
        more[0].effective |= 1u << 21;
        R("capset take it back", capset(&hdr, more));
        R("capget", capget(&hdr, data));
        show_caps("restored", data);
    }
}

static void modules_io_log(void)
{
    printf("== modules, port I/O, kernel log, LDT, quota, accounting, hangup\n");
    R("acct(NULL)", acct(NULL));
    R("acct(missing)", acct("/nonexistent/x"));
    R("init_module(NULL, 0, \"\")", init_module(NULL, 0, ""));
    char img[16] = {0};
    R("init_module(16 zero bytes)", init_module(img, sizeof img, ""));
    R("delete_module(absent)", delete_module("liella_nonesuch", 0));
    R("delete_module(absent, O_NONBLOCK)", delete_module("liella_nonesuch", 04000));
    R("ioperm(0x80, 1, 1)", ioperm(0x80, 1, 1));
    R("ioperm(0x10000, 1, 1)", ioperm(0x10000, 1, 1));
    R("ioperm(0x80, 1, 0)", ioperm(0x80, 1, 0));
    R("iopl(0)", iopl(0));
    R("iopl(3)", iopl(3));
    R("iopl(4)", iopl(4));
    R("iopl(-1)", iopl(-1));
    char kb[64];
    R("klogctl(SIZE_BUFFER)", klogctl(10, NULL, 0));
    R("klogctl(READ_ALL, 64)", klogctl(3, kb, sizeof kb));
    R("klogctl(99)", klogctl(99, NULL, 0));
    unsigned char ldt[64];
    memset(ldt, 0xAA, sizeof ldt);
    R("modify_ldt(read, 64)", modify_ldt(0, ldt, sizeof ldt));
    R("modify_ldt(read default, 64)", modify_ldt(2, ldt, sizeof ldt));
    R("modify_ldt(7)", modify_ldt(7, ldt, sizeof ldt));
    R("modify_ldt(write, 4 bytes)", modify_ldt(1, ldt, 4));
    /* struct user_desc: entry 0, base 0, limit 0xfff, seg_32bit, limit_in_pages, useable. */
    unsigned int desc[4] = {0, 0, 0xfff, 1u | (1u << 4) | (1u << 6)};
    R("modify_ldt(write entry 0)", modify_ldt(1, desc, sizeof desc));
    memset(ldt, 0, sizeof ldt);
    R("modify_ldt(read, 64)", modify_ldt(0, ldt, sizeof ldt));
    printf("  entry 0: %02x%02x%02x%02x%02x%02x%02x%02x\n", ldt[0], ldt[1], ldt[2], ldt[3], ldt[4], ldt[5], ldt[6], ldt[7]);
    R("modify_ldt(read, 0)", modify_ldt(0, ldt, 0));
    R("quotactl(Q_GETINFO, /)", quotactl(QCMD(Q_GETINFO, USRQUOTA), "/", 0, (caddr_t)kb));
    /* No device: the kernel's NULL path. A valid Q_SYNC there is a whole-machine writeback (keke:
       3.8 s with 4 GB dirty; in one shiki gate the reference hung past its watchdog in it), so
       its time is the machine's dirty backlog, not the library's. The invalid quota type 7
       reaches the same NULL path and fails with EINVAL without syncing anything. */
    R("quotactl(Q_SYNC, NULL, type 7)", quotactl(QCMD(Q_SYNC, 7), NULL, 0, NULL));
    R("quotactl(Q_GETINFO, NULL)", quotactl(QCMD(Q_GETINFO, USRQUOTA), NULL, 0, (caddr_t)kb));
    R("quotactl(unknown, /dev/null)", quotactl(QCMD(0x7777, USRQUOTA), "/dev/null", 0, NULL));
    R("quotactl(Q_GETQUOTA, missing)", quotactl(QCMD(Q_GETQUOTA, USRQUOTA), "/nonexistent", 0, (caddr_t)kb));
    R("vhangup()", vhangup());
}

/* Whether this process is in a user namespace other than the initial one. */
static int in_user_namespace(void)
{
    FILE *f = fopen("/proc/self/uid_map", "r");
    if (!f)
        return 0;
    unsigned long inside, outside, count;
    int n = fscanf(f, "%lu %lu %lu", &inside, &outside, &count);
    fclose(f);
    return n == 3 && !(inside == 0 && outside == 0 && count == 4294967295UL);
}

int main(int argc, char **argv)
{
    setvbuf(stdout, NULL, _IONBF, 0);
    const char *mode = argc > 1 ? argv[1] : "plain";
    if (strcmp(mode, "plain") == 0) {
        if (geteuid() == 0) {
            printf("refused: run the plain mode as an unprivileged user\n");
            return 2;
        }
        version();
        capabilities(0);
        modules_io_log();
        printf("== reboot (unprivileged)\n");
        R("reboot(RB_DISABLE_CAD)", reboot(RB_DISABLE_CAD));
        R("reboot(0x7777)", reboot(0x7777));
        return 0;
    }
    if (!in_user_namespace()) {
        printf("refused: the %s mode runs only in a private user namespace\n", mode);
        return 2;
    }
    if (strcmp(mode, "userns") == 0) {
        capabilities(1);
        modules_io_log();
        return 0;
    }
    if (strcmp(mode, "pidns") == 0) {
        printf("== reboot in a private PID namespace (pid %d)\n", getpid());
        if (getpid() != 1) {
            printf("refused: not the namespace's first process\n");
            return 2;
        }
        R("reboot(RB_DISABLE_CAD)", reboot(RB_DISABLE_CAD));
        R("reboot(RB_ENABLE_CAD)", reboot(RB_ENABLE_CAD));
        R("reboot(0x7777)", reboot(0x7777));
        R("reboot(RB_SW_SUSPEND)", reboot(RB_SW_SUSPEND));
        printf("reboot(RB_POWER_OFF): the namespace ends\n");
        reboot(RB_POWER_OFF);
        printf("still running\n");
        return 3;
    }
    printf("unknown mode\n");
    return 2;
}
