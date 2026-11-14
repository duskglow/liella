/* validation/smoke/selftest/nolibc.c: a negative control of smoke.sh selftest. Linked by liella-cc without
   start files and with --as-needed, it has Liella's loader as PT_INTERP but names no C library at all, which
   check-elf.sh must reject (a program on Liella's loader must use Liella's libc). Never run. */
void _start(void) { __builtin_trap(); }
