/* host.conf and RESOLV_* diagnostics (D-16.13): argv[1] "name" does two host-name lookups,
 * "numeric" a getaddrinfo of a numeric host only, "numeric-name" the numeric one and then a name. */
#include <netdb.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>

static void numeric(void) {
    struct addrinfo hints, *r = 0;
    memset(&hints, 0, sizeof hints);
    hints.ai_flags = AI_NUMERICHOST;
    hints.ai_socktype = SOCK_STREAM;
    printf("numeric %d\n", getaddrinfo("127.0.0.1", "80", &hints, &r));
}

static void name(void) {
    struct hostent *h = gethostbyname("localhost");
    printf("name %s\n", h ? "found" : "none");
    h = gethostbyname("localhost");
    printf("name %s\n", h ? "found" : "none");
}

int main(int argc, char **argv) {
    const char *m = argc > 1 ? argv[1] : "name";
    if (strcmp(m, "numeric") == 0 || strcmp(m, "numeric-name") == 0) numeric();
    if (strcmp(m, "name") == 0 || strcmp(m, "numeric-name") == 0) name();
    fflush(stdout);
    return 0;
}
