#!/usr/bin/env python3
# Host-library oracle for the number -> text differential tests (crates/core/src/num/glibc_diff.rs).
# Black box: one record per stdin line, "<kind> <fesetround mode> <conversion> <hex bits>", kind d
# (double, snprintf), f (float, strfromf) or l (x87 long double, strfroml); one output line per record,
# the host library's text. Runs in its own process, so the calls reach the host C library and never
# Liella's exports. Run with python3 -I.
import ctypes
import sys

libc = ctypes.CDLL("libc.so.6")
libm = ctypes.CDLL("libm.so.6")
libc.strfromf.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.c_char_p, ctypes.c_float]
libc.strfroml.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.c_char_p, ctypes.c_longdouble]
SIZE = 16384
buf = ctypes.create_string_buffer(SIZE)
out = sys.stdout.buffer
for line in sys.stdin.buffer:
    kind, mode, conv, bits = line.split()
    bits = int(bits, 16)
    libm.fesetround(int(mode))
    if kind == b"d":
        n = libc.snprintf(buf, SIZE, conv, ctypes.c_double.from_buffer_copy(bits.to_bytes(8, "little")))
    elif kind == b"f":
        n = libc.strfromf(buf, SIZE, conv, ctypes.c_float.from_buffer_copy(bits.to_bytes(4, "little")))
    else:
        n = libc.strfroml(buf, SIZE, conv, ctypes.c_longdouble.from_buffer_copy(bits.to_bytes(16, "little")))
    libm.fesetround(0)
    if n < 0 or n >= SIZE:
        out.write(b"ORACLE-ERROR\n")
    else:
        out.write(buf.raw[:n] + b"\n")
out.flush()
