/* RESOLV_HOST_CONF end to end (D-16.13): print the addresses gethostbyname gives "multi.test".
 * The test binds a private /etc/hosts and /etc/nsswitch.conf (hosts: files) in a user and mount
 * namespace and names the host.conf file in the environment. */
#include <netdb.h>
#include <stdio.h>
#include <arpa/inet.h>

int main(void) {
    struct hostent *h = gethostbyname("multi.test");
    if (!h) {
        printf("none h_errno=%d\n", h_errno);
        return 0;
    }
    for (char **a = h->h_addr_list; *a; a++) {
        char buf[64];
        printf("%s\n", inet_ntop(h->h_addrtype, *a, buf, sizeof buf));
    }
    return 0;
}
