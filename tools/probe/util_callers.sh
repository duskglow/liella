#!/bin/bash
# LIELLA-UTILITIES-DESIGN probe 2: dpkg trigger declarations and how two non-libc callers
# (CPython ctypes, initramfs-tools) consume ldconfig -p and ldd output. Reads no libc script.
set +e
export LC_ALL=C
echo "### triggers"
cd /var/lib/dpkg/info
echo "triggers-files=$(ls *.triggers | wc -l) shlibs-files=$(ls *.shlibs 2>/dev/null | wc -l)"
grep -h -E '^activate' *.triggers | grep -w ldconfig | sed 's/[[:space:]]\+/ /g' | sort | uniq -c
grep -h -E '^interest' *.triggers | sed 's/[[:space:]]\+/ /g' | sort | uniq -c | sort -rn | head -8
echo "conf-owners:"; for f in /etc/ld.so.conf /etc/ld.so.conf.d/*.conf /etc/locale.alias /etc/nsswitch.conf /usr/share/locale/locale.alias; do printf '%s: ' $f; dpkg -S $f 2>/dev/null | cut -d: -f1 | tr '\n' ' '; echo; done
echo "### python-ctypes"
grep -n -E 'ldconfig|abi_type|mach_map|regex' /usr/lib/python3.12/ctypes/util.py | head -24
echo "### initramfs-ldd"
grep -n -B2 -A8 -w 'ldd' /usr/share/initramfs-tools/hook-functions | head -40
echo "### end"
