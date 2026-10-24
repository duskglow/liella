/* psignal and the standard error stream (spec §20.8): each case runs in a forked child whose
 * standard error and standard output are files, and the output and the stream's state after the
 * call are compared with what the host glibc 2.39 did in the same case (probed black-box):
 *   a  fully buffered stderr with pending text "A\n", then the call, then "B\n"
 *   b  wide-oriented stderr (fwide(stderr, 1))
 *   c  fclose(stderr), then a file opened as descriptor 2
 *   d  descriptor 2 closed under an unbuffered stderr
 *   f  a fresh stderr
 * psignal goes through the stream (as perror and warn do); psiginfo and herror write to
 * descriptor 2 behind the stream's back (glibc's observed behaviour). */
#define _GNU_SOURCE
#include <err.h>
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wchar.h>

extern char *program_invocation_short_name;

struct row {
	const char *op;
	char c;
	const char *err;  /* the content of descriptor 2's file; '@' stands for the program name */
	const char *rep;  /* the report line on standard output; NULL: none expected */
	const char *note; /* non-NULL: known difference from glibc, reported but not failed */
};

#define INT "P: Interrupt\n"
#define SI "P: Interrupt (Signal sent by kill() 0 0)\n"
#define ENO "P: No such file or directory\n"
#define WARN "@: P: No such file or directory\n"

static const struct row rows[] = {
	{"psignal", 'a', "A\n" INT "B\n", NULL, NULL},
	{"psignal", 'b', INT, "[ferror=0 fwide=1 errno=2]\n", NULL},
	{"psignal", 'c', "", NULL, NULL},
	{"psignal", 'd', "", "[ferror=1 fwide=-1 errno=9]\n", NULL},
	{"psignal", 'f', INT, "[ferror=0 fwide=-1 errno=2]\n", NULL},
	{"psiginfo", 'a', SI "A\nB\n", NULL, NULL},
	{"psiginfo", 'b', SI, "[ferror=0 fwide=1 errno=2]\n", NULL},
	{"psiginfo", 'c', SI, NULL, NULL},
	{"psiginfo", 'd', "", "[ferror=0 fwide=0 errno=9]\n", NULL},
	{"psiginfo", 'f', SI, "[ferror=0 fwide=0 errno=2]\n", NULL},
	{"perror", 'a', "A\n" ENO "B\n", NULL, NULL},
	{"perror", 'b', ENO, "[ferror=0 fwide=1 errno=2]\n", NULL},
	{"perror", 'c', "", NULL, NULL},
	{"perror", 'd', "", "[ferror=1 fwide=-1 errno=9]\n", "(perror does not orient an undecided stream)"},
	{"perror", 'f', ENO, "[ferror=0 fwide=-1 errno=22]\n", "(perror orientation; glibc leaves errno 22, not followed)"},
	{"herror", 'a', "P: Unknown host\nA\nB\n", NULL, "(herror goes through the stream)"},
	{"herror", 'b', "P: Unknown host\n", "[ferror=0 fwide=1 errno=2]\n", NULL},
	{"herror", 'c', "P: Unknown host\n", NULL, "(herror goes through the stream)"},
	{"herror", 'd', "", "[ferror=0 fwide=0 errno=2]\n", "(herror sets the error flag)"},
	{"herror", 'f', "P: Unknown host\n", "[ferror=0 fwide=0 errno=2]\n", NULL},
	{"warn", 'a', "A\n" WARN "B\n", NULL, NULL},
	{"warn", 'b', WARN, "[ferror=0 fwide=1 errno=2]\n", NULL},
	{"warn", 'c', "", NULL, NULL},
	{"warn", 'd', "", "[ferror=1 fwide=-1 errno=9]\n", NULL},
	{"warn", 'f', WARN, "[ferror=0 fwide=-1 errno=2]\n", NULL},
	{"warnx", 'a', "A\n@: P\nB\n", NULL, NULL},
	{"warnx", 'c', "", NULL, NULL},
	{"err", 'a', "A\n" WARN, NULL, NULL},
	{"err", 'c', "", NULL, NULL},
};

static const char *op;

static void call(void)
{
	siginfo_t si;
	memset(&si, 0, sizeof si);
	si.si_signo = SIGINT;
	si.si_code = SI_USER;
	errno = ENOENT;
	h_errno = HOST_NOT_FOUND;
	if (!strcmp(op, "psignal")) psignal(SIGINT, "P");
	else if (!strcmp(op, "psiginfo")) psiginfo(&si, "P");
	else if (!strcmp(op, "perror")) perror("P");
	else if (!strcmp(op, "herror")) herror("P");
	else if (!strcmp(op, "warn")) warn("P");
	else if (!strcmp(op, "warnx")) warnx("P");
	else if (!strcmp(op, "err")) err(3, "P");
}

static void report(void)
{
	char b[80];
	int n = snprintf(b, sizeof b, "[ferror=%d fwide=%d errno=%d]\n", ferror(stderr), fwide(stderr, 0), errno);
	(void)!write(1, b, (size_t)n);
}

static void child(char c, const char *errpath, const char *outpath)
{
	int o = open(outpath, O_WRONLY | O_CREAT | O_TRUNC, 0600);
	if (o < 0 || dup2(o, 1) < 0) _exit(90);
	if (c == 'c') {
		fclose(stderr);
		if (open(errpath, O_WRONLY | O_CREAT | O_TRUNC, 0600) != 2) _exit(91);
		call();
		exit(0);
	}
	int e = open(errpath, O_WRONLY | O_CREAT | O_TRUNC, 0600);
	if (e < 0 || dup2(e, 2) < 0) _exit(92);
	if (c == 'a') {
		static char buf[4096];
		setvbuf(stderr, buf, _IOFBF, sizeof buf);
		fputs("A\n", stderr);
		call();
		fputs("B\n", stderr);
	} else if (c == 'b') {
		fwide(stderr, 1);
		call();
		report();
	} else if (c == 'd') {
		close(2);
		call();
		report();
	} else {
		call();
		report();
	}
	exit(0);
}

static size_t slurp(const char *path, char *buf, size_t cap)
{
	int fd = open(path, O_RDONLY);
	size_t n = 0;
	if (fd >= 0) {
		ssize_t r;
		while (n + 1 < cap && (r = read(fd, buf + n, cap - 1 - n)) > 0) n += (size_t)r;
		close(fd);
	}
	buf[n] = 0;
	return n;
}

static void expand(const char *s, char *out, size_t cap)
{
	size_t n = 0;
	for (; *s && n + 64 < cap; s++) {
		if (*s == '@') {
			strcpy(out + n, program_invocation_short_name);
			n += strlen(program_invocation_short_name);
		} else out[n++] = *s;
	}
	out[n] = 0;
}

int main(void)
{
	const char *tmp = getenv("TMPDIR");
	char errpath[256], outpath[256];
	int bad = 0;
	snprintf(errpath, sizeof errpath, "%s/psigstr_e.%d", tmp ? tmp : "/tmp", (int)getpid());
	snprintf(outpath, sizeof outpath, "%s/psigstr_o.%d", tmp ? tmp : "/tmp", (int)getpid());
	for (size_t i = 0; i < sizeof rows / sizeof rows[0]; i++) {
		const struct row *r = &rows[i];
		char ge[512], go[512], we[512];
		int st = 0;
		fflush(NULL);
		op = r->op;
		pid_t p = fork();
		if (p == 0) child(r->c, errpath, outpath);
		waitpid(p, &st, 0);
		slurp(errpath, ge, sizeof ge);
		slurp(outpath, go, sizeof go);
		expand(r->err, we, sizeof we);
		int want_status = !strcmp(r->op, "err") ? 3 : 0;
		int ok = WIFEXITED(st) && WEXITSTATUS(st) == want_status && !strcmp(ge, we) && (r->rep == NULL || !strcmp(go, r->rep));
		if (ok) continue;
		printf("%s %s %s case %c: status=%d err=[%s] want=[%s] rep=[%s] want=[%s]\n", r->note ? "KNOWN-DIFF" : "MISMATCH", r->op,
		       r->note ? r->note : "", r->c, st, ge, we, go, r->rep ? r->rep : "-");
		if (!r->note) bad++;
	}
	unlink(errpath);
	unlink(outpath);
	if (bad) {
		printf("%d mismatches\n", bad);
		return 1;
	}
	return 0;
}
