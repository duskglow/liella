/* What the resolver takes from the environment in secure-execution mode (owner ruling
 * 2026-10-03; spec/security/threat-model.md): HOSTALIASES, RES_OPTIONS and LOCALDOMAIN steer the
 * resolver of an ordinary process and are ignored when AT_SECURE is set. Run by
 * tests/secure_env.rs with all three set, once as a set-group-ID copy and once plain. Prints only
 * their effects: the reference library also removes the variables from the environment of a
 * secure process, which this probe does not compare. */
#include <stdio.h>
#pragma GCC diagnostic ignored "-Wdeprecated-declarations"
#include <sys/auxv.h>
#include <resolv.h>

const char *hostalias(const char *);

int main(void)
{
    printf("AT_SECURE=%lu\n", getauxval(AT_SECURE));
    res_init();
    printf("ndots=%u\n", (unsigned)_res.ndots);
    printf("search[0] is LOCALDOMAIN's: %s\n", _res.dnsrch[0] && _res.dnsrch[0][0] == 'e' && _res.dnsrch[0][1] == 'v' ? "yes" : "no");
    const char *a = hostalias("short");
    printf("hostalias(short)=%s\n", a ? a : "(null)");
    return 0;
}
