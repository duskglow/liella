# Binary-compatibility sweep (LG-0c)

Roots: /usr/bin, /usr/sbin, /usr/lib, /usr/lib64, /usr/libexec

| count | value |
|---|---|
| files | 33501 |
| elf | 2811 |
| foreign | 2 |
| nodyn | 620 |
| glibc_components | 259 |
| dynamic | 1930 |
| programs | 1036 |
| unversioned | 2 |
| distinct glibc sym@VER needed | 1358 |
| ... of kind default | 1358 |
| distinct compat sym@VER needed | 0 |
| distinct needed sym@VER Liella lacks | 17 |
| programs ok directly | 973 of 1036 |
| programs ok with their libraries | 483 of 1036 |
| libraries/objects ok directly | 744 of 894 |

## Top 25 compat needs (ELF users)


## Top 25 program blockers (programs blocked directly)

- sym:dlopen@GLIBC_2.34: 42
- sym:dlsym@GLIBC_2.34: 41
- sym:dlerror@GLIBC_2.34: 35
- sym:dlclose@GLIBC_2.34: 32
- lib:ld-linux-x86-64.so.2: 17
- sym:__tls_get_addr@GLIBC_2.3: 17
- node:libc.so.6@GLIBC_ABI_DT_RELR: 8
- lib:libresolv.so.2: 2
- sym:__b64_ntop@GLIBC_2.2.5: 2
- sym:__b64_pton@GLIBC_2.2.5: 1
- sym:__profile_frequency@GLIBC_2.2.5: 1
- sym:_mcleanup@GLIBC_2.2.5: 1
- sym:dladdr@GLIBC_2.34: 1
- sym:monstartup@GLIBC_2.2.5: 1

## Top 25 blockers of programs that are ok directly but not with their libraries

- via a library: lib:ld-linux-x86-64.so.2: 354
- via a library: sym:dlerror@GLIBC_2.34: 111
- via a library: sym:dlclose@GLIBC_2.34: 9
- via a library: sym:dlopen@GLIBC_2.34: 5
- via a library: sym:dlsym@GLIBC_2.34: 5
- via a library: sym:dlvsym@GLIBC_2.34: 3
- via a library: lib:libresolv.so.2: 2
- via a library: sym:dladdr@GLIBC_2.34: 1
