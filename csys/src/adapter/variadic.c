/* Class adapter (proposed, OD-C1): variadic entry points of variadic exports, each a single
 * forwarding call, e.g. printf(fmt, ...) -> vprintf(fmt, ap). Names are the exported glibc
 * names, compiled only under LIELLA_EXPORT. Added in phases 5 and 8; empty until then. */
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include "liella_csys.h"

/* The exported glibc names in the products; `liella_vfwd_<name>` in test builds, so that tests
 * never interpose on the host C library (the forwarders still compile). */
#ifdef LIELLA_EXPORT
#define LIELLA_VFWD(name) name
#else
#define LIELLA_VFWD(name) liella_vfwd_##name
#endif

/* §11 (phase 5): open, openat, fcntl, ioctl, mremap. The optional argument is read
 * unconditionally: an absent `int`/pointer argument is an unspecified register value on x86-64,
 * which the Rust side ignores for the commands that take none. */
int liella_open3(const char *path, int flags, unsigned mode);
int liella_openat4(int dirfd, const char *path, int flags, unsigned mode);
int liella_fcntl3(int fd, int cmd, uintptr_t arg);
int liella_ioctl3(int fd, unsigned long request, uintptr_t arg);
void *liella_mremap5(void *old, size_t old_len, size_t new_len, int flags, void *new_addr);
#define OPEN3(name) int name(const char *path, int flags, ...) { va_list ap; va_start(ap, flags); int r = liella_open3(path, flags, va_arg(ap, unsigned)); va_end(ap); return r; }
#define OPENAT4(name) int name(int dirfd, const char *path, int flags, ...) { va_list ap; va_start(ap, flags); int r = liella_openat4(dirfd, path, flags, va_arg(ap, unsigned)); va_end(ap); return r; }
#define FCNTL3(name) int name(int fd, int cmd, ...) { va_list ap; va_start(ap, cmd); int r = liella_fcntl3(fd, cmd, va_arg(ap, uintptr_t)); va_end(ap); return r; }
OPEN3(LIELLA_VFWD(open))
OPEN3(LIELLA_VFWD(open64))
OPEN3(LIELLA_VFWD(__open))
OPEN3(LIELLA_VFWD(__open64))
OPENAT4(LIELLA_VFWD(openat))
OPENAT4(LIELLA_VFWD(openat64))
FCNTL3(LIELLA_VFWD(fcntl))
FCNTL3(LIELLA_VFWD(fcntl64))
FCNTL3(LIELLA_VFWD(__fcntl))
int LIELLA_VFWD(ioctl)(int fd, unsigned long request, ...) { va_list ap; va_start(ap, request); int r = liella_ioctl3(fd, request, va_arg(ap, uintptr_t)); va_end(ap); return r; }
void *LIELLA_VFWD(mremap)(void *old, size_t old_len, size_t new_len, int flags, ...) { va_list ap; va_start(ap, flags); void *r = liella_mremap5(old, old_len, new_len, flags, va_arg(ap, void *)); va_end(ap); return r; }

/* §25.2 (phase 5): semctl's fourth argument, a `union semun` passed by value (one register on
 * x86-64), read unconditionally as above. */
int liella_semctl4(int id, int num, int cmd, uintptr_t arg);
int LIELLA_VFWD(semctl)(int id, int num, int cmd, ...) { va_list ap; va_start(ap, cmd); int r = liella_semctl4(id, num, cmd, va_arg(ap, uintptr_t)); va_end(ap); return r; }

/* §24 (phase 5): ulimit's optional new limit, and syscall's six register-sized arguments, read
 * in order (one statement each: C leaves the order of a call's argument evaluation open). */
long liella_ulimit2(int cmd, long newlimit);
long liella_syscall7(long nr, long a, long b, long c, long d, long e, long f);
long LIELLA_VFWD(ulimit)(int cmd, ...) { va_list ap; va_start(ap, cmd); long r = liella_ulimit2(cmd, va_arg(ap, long)); va_end(ap); return r; }
long LIELLA_VFWD(syscall)(long nr, ...)
{
    va_list ap;
    va_start(ap, nr);
    long a = va_arg(ap, long);
    long b = va_arg(ap, long);
    long c = va_arg(ap, long);
    long d = va_arg(ap, long);
    long e = va_arg(ap, long);
    long f = va_arg(ap, long);
    va_end(ap);
    return liella_syscall7(nr, a, b, c, d, e, f);
}

/* §21.2 (phase 6): makecontext's int arguments. The va_list goes to Rust, which reads exactly
 * argc arguments through the accessor below (one va_arg per call). */
void liella_makecontext4(void *uc, uintptr_t fn, int argc, va_list *ap);
void LIELLA_VFWD(makecontext)(void *uc, void (*fn)(void), int argc, ...) { va_list ap; va_start(ap, argc); liella_makecontext4(uc, (uintptr_t)fn, argc, &ap); va_end(ap); }

/* The va_arg accessors (liella_va_long, ...) are in va.c. */

/* §22.7 (phase 6): sem_open's mode and initial value, read unconditionally (meaningful with
 * O_CREAT; an absent argument is an unspecified register the Rust side ignores). */
void *liella_sem_open4(const char *name, int oflag, unsigned mode, unsigned value);
void *LIELLA_VFWD(sem_open)(const char *name, int oflag, ...)
{
    va_list ap;
    va_start(ap, oflag);
    unsigned mode = va_arg(ap, unsigned);
    unsigned value = va_arg(ap, unsigned);
    va_end(ap);
    return liella_sem_open4(name, oflag, mode, value);
}

/* §18.2 (phase 6): clone's optional ptid, tls and ctid, read unconditionally (used only as the
 * flags ask). */
int liella_clone7(uintptr_t fn, uintptr_t stack, int flags, uintptr_t arg, uintptr_t ptid, uintptr_t tls, uintptr_t ctid);
#define CLONE7(name) \
int name(int (*fn)(void *), void *stack, int flags, void *arg, ...) \
{ \
    va_list ap; \
    va_start(ap, arg); \
    uintptr_t ptid = va_arg(ap, uintptr_t); \
    uintptr_t tls = va_arg(ap, uintptr_t); \
    uintptr_t ctid = va_arg(ap, uintptr_t); \
    va_end(ap); \
    return liella_clone7((uintptr_t)fn, (uintptr_t)stack, flags, (uintptr_t)arg, ptid, tls, ctid); \
}
CLONE7(LIELLA_VFWD(clone))
CLONE7(LIELLA_VFWD(__clone))

/* §18.3 (phase 11): the l forms of exec. The va_list goes to Rust, which reads the arguments
 * after arg0 up to the NULL (and, for execle, the environment after it) through
 * liella_va_long. `form`: 0 execl, 1 execle, 2 execlp. */
int liella_execl4(int form, const char *path, const char *arg0, va_list *ap);
int LIELLA_VFWD(execl)(const char *path, const char *arg, ...) { va_list ap; va_start(ap, arg); int r = liella_execl4(0, path, arg, &ap); va_end(ap); return r; }
int LIELLA_VFWD(execle)(const char *path, const char *arg, ...) { va_list ap; va_start(ap, arg); int r = liella_execl4(1, path, arg, &ap); va_end(ap); return r; }
int LIELLA_VFWD(execlp)(const char *file, const char *arg, ...) { va_list ap; va_start(ap, arg); int r = liella_execl4(2, file, arg, &ap); va_end(ap); return r; }

/* §18.9 (phase 11): prctl's four further arguments (unsigned long, prctl(2)) and ptrace's pid,
 * addr and data, read unconditionally (an option or request that takes fewer ignores the
 * rest), one statement each. */
int liella_prctl5(int option, unsigned long a2, unsigned long a3, unsigned long a4, unsigned long a5);
int LIELLA_VFWD(prctl)(int option, ...)
{
    va_list ap;
    va_start(ap, option);
    unsigned long a2 = va_arg(ap, unsigned long);
    unsigned long a3 = va_arg(ap, unsigned long);
    unsigned long a4 = va_arg(ap, unsigned long);
    unsigned long a5 = va_arg(ap, unsigned long);
    va_end(ap);
    return liella_prctl5(option, a2, a3, a4, a5);
}
long liella_ptrace4(int request, int pid, unsigned long addr, unsigned long data);
long LIELLA_VFWD(ptrace)(int request, ...)
{
    va_list ap;
    va_start(ap, request);
    int pid = va_arg(ap, int);
    unsigned long addr = va_arg(ap, unsigned long);
    unsigned long data = va_arg(ap, unsigned long);
    va_end(ap);
    return liella_ptrace4(request, pid, addr, data);
}

/* §12.6 (phase 8): the printf family. Each variadic name forwards its fixed arguments and its
 * va_list (by address) to the private Rust entry of its destination; `flag` is the fortified
 * forms' check level (0 for the plain forms), `slen` their destination size ((size_t)-1: none),
 * and a NULL stream means `stdout`. */
int liella_vfprintf4(void *fp, int flag, const char *fmt, void *ap);
int liella_vdprintf4(int fd, int flag, const char *fmt, void *ap);
int liella_vsnprintf6(char *s, size_t maxlen, int flag, size_t slen, const char *fmt, void *ap);
int liella_vasprintf4(char **strp, int flag, const char *fmt, void *ap);
int liella_obstack_vprintf4(void *obs, int flag, const char *fmt, void *ap);
int liella_vfwprintf4(void *fp, int flag, const int *fmt, void *ap);
int liella_vswprintf6(int *s, size_t maxlen, int flag, size_t slen, const int *fmt, void *ap);

#define FWD(last, call) \
{ \
    va_list ap; \
    va_start(ap, last); \
    int r = call; \
    va_end(ap); \
    return r; \
}
int LIELLA_VFWD(printf)(const char *fmt, ...) FWD(fmt, liella_vfprintf4(NULL, 0, fmt, &ap))
int LIELLA_VFWD(_IO_printf)(const char *fmt, ...) FWD(fmt, liella_vfprintf4(NULL, 0, fmt, &ap))
int LIELLA_VFWD(fprintf)(void *fp, const char *fmt, ...) FWD(fmt, liella_vfprintf4(fp, 0, fmt, &ap))
int LIELLA_VFWD(_IO_fprintf)(void *fp, const char *fmt, ...) FWD(fmt, liella_vfprintf4(fp, 0, fmt, &ap))
int LIELLA_VFWD(dprintf)(int fd, const char *fmt, ...) FWD(fmt, liella_vdprintf4(fd, 0, fmt, &ap))
int LIELLA_VFWD(sprintf)(char *s, const char *fmt, ...) FWD(fmt, liella_vsnprintf6(s, (size_t)-1, 0, (size_t)-1, fmt, &ap))
int LIELLA_VFWD(_IO_sprintf)(char *s, const char *fmt, ...) FWD(fmt, liella_vsnprintf6(s, (size_t)-1, 0, (size_t)-1, fmt, &ap))
int LIELLA_VFWD(snprintf)(char *s, size_t n, const char *fmt, ...) FWD(fmt, liella_vsnprintf6(s, n, 0, (size_t)-1, fmt, &ap))
int LIELLA_VFWD(asprintf)(char **strp, const char *fmt, ...) FWD(fmt, liella_vasprintf4(strp, 0, fmt, &ap))
int LIELLA_VFWD(__asprintf)(char **strp, const char *fmt, ...) FWD(fmt, liella_vasprintf4(strp, 0, fmt, &ap))
int LIELLA_VFWD(obstack_printf)(void *obs, const char *fmt, ...) FWD(fmt, liella_obstack_vprintf4(obs, 0, fmt, &ap))
int LIELLA_VFWD(wprintf)(const int *fmt, ...) FWD(fmt, liella_vfwprintf4(NULL, 0, fmt, &ap))
int LIELLA_VFWD(fwprintf)(void *fp, const int *fmt, ...) FWD(fmt, liella_vfwprintf4(fp, 0, fmt, &ap))
int LIELLA_VFWD(swprintf)(int *s, size_t n, const int *fmt, ...) FWD(fmt, liella_vswprintf6(s, n, 0, (size_t)-1, fmt, &ap))
int LIELLA_VFWD(__printf_chk)(int flag, const char *fmt, ...) FWD(fmt, liella_vfprintf4(NULL, flag, fmt, &ap))
int LIELLA_VFWD(__fprintf_chk)(void *fp, int flag, const char *fmt, ...) FWD(fmt, liella_vfprintf4(fp, flag, fmt, &ap))
int LIELLA_VFWD(__dprintf_chk)(int fd, int flag, const char *fmt, ...) FWD(fmt, liella_vdprintf4(fd, flag, fmt, &ap))
int LIELLA_VFWD(__sprintf_chk)(char *s, int flag, size_t slen, const char *fmt, ...) FWD(fmt, liella_vsnprintf6(s, (size_t)-1, flag, slen, fmt, &ap))
int LIELLA_VFWD(__snprintf_chk)(char *s, size_t maxlen, int flag, size_t slen, const char *fmt, ...) FWD(fmt, liella_vsnprintf6(s, maxlen, flag, slen, fmt, &ap))
int LIELLA_VFWD(__asprintf_chk)(char **strp, int flag, const char *fmt, ...) FWD(fmt, liella_vasprintf4(strp, flag, fmt, &ap))
int LIELLA_VFWD(__obstack_printf_chk)(void *obs, int flag, const char *fmt, ...) FWD(fmt, liella_obstack_vprintf4(obs, flag, fmt, &ap))
int LIELLA_VFWD(__wprintf_chk)(int flag, const int *fmt, ...) FWD(fmt, liella_vfwprintf4(NULL, flag, fmt, &ap))
int LIELLA_VFWD(__fwprintf_chk)(void *fp, int flag, const int *fmt, ...) FWD(fmt, liella_vfwprintf4(fp, flag, fmt, &ap))
int LIELLA_VFWD(__swprintf_chk)(int *s, size_t n, int flag, size_t slen, const int *fmt, ...) FWD(fmt, liella_vswprintf6(s, n, flag, slen, fmt, &ap))

/* §12.7 (phase 8): the scanf family. `flavour` is the symbol set (0 plain: GNU `%a` allocates;
 * 1 `__isoc99_`; 2 `__isoc23_`: `0b` prefixes); a NULL stream means `stdin`. */
int liella_vfscanf4(void *fp, int flavour, const char *fmt, void *ap);
int liella_vsscanf4(const char *s, int flavour, const char *fmt, void *ap);
int liella_vfwscanf4(void *fp, int flavour, const int *fmt, void *ap);
int liella_vswscanf4(const int *s, int flavour, const int *fmt, void *ap);
#define SCANF6(p, f) \
int LIELLA_VFWD(p##scanf)(const char *fmt, ...) FWD(fmt, liella_vfscanf4(NULL, f, fmt, &ap)) \
int LIELLA_VFWD(p##fscanf)(void *fp, const char *fmt, ...) FWD(fmt, liella_vfscanf4(fp, f, fmt, &ap)) \
int LIELLA_VFWD(p##sscanf)(const char *s, const char *fmt, ...) FWD(fmt, liella_vsscanf4(s, f, fmt, &ap)) \
int LIELLA_VFWD(p##wscanf)(const int *fmt, ...) FWD(fmt, liella_vfwscanf4(NULL, f, fmt, &ap)) \
int LIELLA_VFWD(p##fwscanf)(void *fp, const int *fmt, ...) FWD(fmt, liella_vfwscanf4(fp, f, fmt, &ap)) \
int LIELLA_VFWD(p##swscanf)(const int *s, const int *fmt, ...) FWD(fmt, liella_vswscanf4(s, f, fmt, &ap))
SCANF6(, 0)
SCANF6(__isoc99_, 1)
SCANF6(__isoc23_, 2)
int LIELLA_VFWD(_IO_sscanf)(const char *s, const char *fmt, ...) FWD(fmt, liella_vsscanf4(s, 0, fmt, &ap))

/* §2.3 (phase 8): err, errx, warn, warnx, error, error_at_line. `kind` names the function
 * (0 warn, 1 warnx, 2 err, 3 errx, 4 error, 5 error_at_line). The err forms do not return. */
void liella_report7(int kind, int status, int errnum, const char *file, unsigned line, const char *fmt, void *ap);
#define REPORT(last, call) \
{ \
    va_list ap; \
    va_start(ap, last); \
    call; \
    va_end(ap); \
}
void LIELLA_VFWD(warn)(const char *fmt, ...) REPORT(fmt, liella_report7(0, 0, 0, NULL, 0, fmt, &ap))
void LIELLA_VFWD(warnx)(const char *fmt, ...) REPORT(fmt, liella_report7(1, 0, 0, NULL, 0, fmt, &ap))
void LIELLA_VFWD(err)(int status, const char *fmt, ...) REPORT(fmt, liella_report7(2, status, 0, NULL, 0, fmt, &ap))
void LIELLA_VFWD(errx)(int status, const char *fmt, ...) REPORT(fmt, liella_report7(3, status, 0, NULL, 0, fmt, &ap))
void LIELLA_VFWD(error)(int status, int errnum, const char *fmt, ...) REPORT(fmt, liella_report7(4, status, errnum, NULL, 0, fmt, &ap))
void LIELLA_VFWD(error_at_line)(int status, int errnum, const char *file, unsigned line, const char *fmt, ...) REPORT(fmt, liella_report7(5, status, errnum, file, line, fmt, &ap))

/* §7.5 (phase 8): strfmon, strfmon_l, __strfmon_l. A NULL locale means the calling thread's. */
long liella_vstrfmon5(char *s, size_t max, void *loc, const char *fmt, void *ap);
#define STRFMON(last, call) \
{ \
    va_list ap; \
    va_start(ap, last); \
    long r = call; \
    va_end(ap); \
    return r; \
}
long LIELLA_VFWD(strfmon)(char *s, size_t max, const char *fmt, ...) STRFMON(fmt, liella_vstrfmon5(s, max, NULL, fmt, &ap))
long LIELLA_VFWD(strfmon_l)(char *s, size_t max, void *loc, const char *fmt, ...) STRFMON(fmt, liella_vstrfmon5(s, max, loc, fmt, &ap))
long LIELLA_VFWD(__strfmon_l)(char *s, size_t max, void *loc, const char *fmt, ...) STRFMON(fmt, liella_vstrfmon5(s, max, loc, fmt, &ap))

/* §1.7 (phase 8): argp_error and argp_failure. `state` is the parser's `struct argp_state *`
 * (NULL allowed). argp_failure returns when the state's flags or a zero status say so. */
void liella_argp_error3(const void *state, const char *fmt, void *ap);
void liella_argp_failure5(const void *state, int status, int errnum, const char *fmt, void *ap);
void LIELLA_VFWD(argp_error)(const void *state, const char *fmt, ...) REPORT(fmt, liella_argp_error3(state, fmt, &ap))
void LIELLA_VFWD(argp_failure)(const void *state, int status, int errnum, const char *fmt, ...) REPORT(fmt, liella_argp_failure5(state, status, errnum, fmt, &ap))

/* §25.3 (phase 13): mq_open's mode and attributes, read unconditionally (meaningful with
 * O_CREAT; absent arguments are unspecified register values the Rust side does not read). */
int liella_mq_open4(const char *name, int oflag, unsigned mode, const void *attr);
int LIELLA_VFWD(mq_open)(const char *name, int oflag, ...)
{
    va_list ap;
    va_start(ap, oflag);
    unsigned mode = va_arg(ap, unsigned);
    const void *attr = va_arg(ap, const void *);
    va_end(ap);
    return liella_mq_open4(name, oflag, mode, attr);
}

/* §26.1 (phase 13): syslog and __syslog_chk. `flag` is the checking form's level (0 for
 * syslog). */
void liella_vsyslog4(int pri, int flag, const char *fmt, void *ap);
void LIELLA_VFWD(syslog)(int pri, const char *fmt, ...) REPORT(fmt, liella_vsyslog4(pri, 0, fmt, &ap))
void LIELLA_VFWD(__syslog_chk)(int pri, int flag, const char *fmt, ...) REPORT(fmt, liella_vsyslog4(pri, flag, fmt, &ap))
