/* The C syscall veneer's contract, checked from outside (COLD-REWRITE-W-far-08).
 *
 * The program is linked with the veneer's own C files (time, system, sched, socket, process,
 * signal) and a header of rows made by the test from csys/syscalls.tsv:
 *     { name, wrapper, kernel number (from the host's asm/unistd.h), argument count }.
 * It calls every wrapper with six distinct 64-bit values under a seccomp filter that traps
 * the veneer's system calls (SECCOMP_RET_TRAP, delivered as SIGSYS). The handler records the
 * number and the six argument registers the kernel would have seen and answers with a chosen
 * result, so no call has any effect (exit, reboot, kill, module loading are all safe to try).
 *
 * Checked per wrapper, in two passes (a positive 64-bit result, then -ENOSYS):
 *   - exactly one system call was issued, with the kernel's number for that wrapper;
 *   - the first N argument registers hold the wrapper's N arguments, in order (x86-64 kernel
 *     order rdi rsi rdx r10 r8 r9; the fourth is r10, not rcx);
 *   - the wrapper returns the kernel's result unchanged: no errno translation, no narrowing
 *     to 32 bits.
 * The generic `syscall` wrapper takes the number first and the six arguments after it.
 *
 * Output on stderr: "veneer_abi: rows=N failures=M". Exit status 0 when M is 0.
 * When no seccomp filter can be installed: "SKIP: <reason>" on stderr and exit 77.
 */
#define _GNU_SOURCE
#include <asm/unistd.h>
#include <errno.h>
#include <linux/audit.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <signal.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <ucontext.h>
#include <unistd.h>

struct row {
	const char *name;
	long (*fn)();
	long nr; /* -1: the generic `syscall` wrapper */
	int nargs;
};

#include VENEER_ROWS

#define NROWS (sizeof rows / sizeof rows[0])
#define EXIT_COOKIE 0x5afe0000u

static volatile int g_count;
static volatile long g_nr;
static volatile unsigned long g_args[6];
static volatile long g_ret;

static void on_sys(int sig, siginfo_t *si, void *ucv)
{
	greg_t *r = ((ucontext_t *)ucv)->uc_mcontext.gregs;
	(void)sig;
	g_count++;
	g_nr = si->si_syscall;
	g_args[0] = (unsigned long)r[REG_RDI];
	g_args[1] = (unsigned long)r[REG_RSI];
	g_args[2] = (unsigned long)r[REG_RDX];
	g_args[3] = (unsigned long)r[REG_R10];
	g_args[4] = (unsigned long)r[REG_R8];
	g_args[5] = (unsigned long)r[REG_R9];
	r[REG_RAX] = g_ret;
}

static int install(void)
{
	/* Layout: arch check, exit_group with the cookie allowed, one JEQ per row, ALLOW, TRAP. */
	struct sock_filter f[NROWS + 16];
	size_t n = 0, i, allow, trap;
	struct sock_fprog prog;
	struct sigaction sa;

	memset(&sa, 0, sizeof sa);
	sa.sa_sigaction = on_sys;
	sa.sa_flags = SA_SIGINFO;
	sigemptyset(&sa.sa_mask);
	if (sigaction(SIGSYS, &sa, NULL) != 0)
		return -1;
	f[n++] = (struct sock_filter)BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, arch));
	f[n++] = (struct sock_filter)BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0);
	f[n++] = (struct sock_filter)BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS);
	f[n++] = (struct sock_filter)BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr));
	f[n++] = (struct sock_filter)BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_exit_group, 0, 3); /* not exit_group: on to the reload */
	f[n++] = (struct sock_filter)BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, args[0]));
	f[n++] = (struct sock_filter)BPF_STMT(BPF_ALU | BPF_AND | BPF_K, 0xffffff00u);
	f[n++] = (struct sock_filter)BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, EXIT_COOKIE, 0, 0); /* jt patched */
	f[n++] = (struct sock_filter)BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr));
	size_t cookie_jump = 7;
	size_t first = n;
	for (i = 0; i < NROWS; i++) {
		long nr = rows[i].nr;
		if (nr < 0)
			continue;
		f[n++] = (struct sock_filter)BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, (unsigned)nr, 0, 0);
	}
	allow = n;
	f[n++] = (struct sock_filter)BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW);
	trap = n;
	f[n++] = (struct sock_filter)BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_TRAP);
	for (i = first; i < allow; i++)
		f[i].jt = (unsigned char)(trap - i - 1);
	f[cookie_jump].jt = (unsigned char)(allow - cookie_jump - 1);
	prog.len = (unsigned short)n;
	prog.filter = f;
	if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0)
		return -1;
	if (prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &prog) != 0)
		return -1;
	return 0;
}

static long call(const struct row *w, const long *v)
{
	if (w->nr < 0)
		return w->fn((long)__NR_sched_getparam, v[0], v[1], v[2], v[3], v[4], v[5]);
	return w->fn(v[0], v[1], v[2], v[3], v[4], v[5]);
}

int main(void)
{
	long v[6];
	size_t i, k;
	int pass, failures = 0;

	for (k = 0; k < 6; k++)
		v[k] = (long)(0x0102030405060708UL * (k + 1) + 0x1111UL * (k + 1) * (k + 7));
	if (install() != 0) {
		fprintf(stderr, "SKIP: cannot install a seccomp filter: %s\n", strerror(errno));
		return 77;
	}
	for (i = 0; i < NROWS; i++) {
		const struct row *w = &rows[i];
		long want_nr = w->nr < 0 ? (long)__NR_sched_getparam : w->nr;
		int nargs = w->nr < 0 ? 6 : w->nargs;
		for (pass = 0; pass < 2; pass++) {
			long want = pass == 0 ? (0x1234567800000000L | want_nr) : -(long)ENOSYS;
			long got;
			g_count = 0;
			g_nr = -2;
			g_ret = want;
			got = call(w, v);
			if (g_count != 1 || g_nr != want_nr) {
				fprintf(stderr, "FAIL %s: %d calls, number %ld, want 1 call of %ld\n", w->name, g_count, (long)g_nr, want_nr);
				failures++;
				break;
			}
			for (k = 0; k < (size_t)nargs; k++) {
				if (g_args[k] != (unsigned long)v[k]) {
					fprintf(stderr, "FAIL %s: kernel argument %zu is %#lx, want %#lx\n", w->name, k, g_args[k], (unsigned long)v[k]);
					failures++;
					pass = 2;
					break;
				}
			}
			if (pass == 2)
				break;
			if (got != want) {
				fprintf(stderr, "FAIL %s: returned %#lx, kernel gave %#lx\n", w->name, got, want);
				failures++;
				break;
			}
		}
	}
	fprintf(stderr, "veneer_abi: rows=%zu failures=%d\n", (size_t)NROWS, failures);
	fflush(stderr);
	syscall(__NR_exit_group, (long)(EXIT_COOKIE | (failures ? 1u : 0u)));
	return 1;
}
