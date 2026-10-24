//! Error texts (spec §2.2; D-2.2): the name and description of each error number, as the
//! reference library gives them in the C locale (recorded in `spec/abi/errmsgs.tsv`).

/// The names and descriptions of the error numbers 0..=133, `None` where the number has none;
/// each NUL-terminated, so that C callers can be given them.
const ERRORS: [Option<(&str, &str)>; 134] = [
    Some(("0\0", "Success\0")),
    Some(("EPERM\0", "Operation not permitted\0")),
    Some(("ENOENT\0", "No such file or directory\0")),
    Some(("ESRCH\0", "No such process\0")),
    Some(("EINTR\0", "Interrupted system call\0")),
    Some(("EIO\0", "Input/output error\0")),
    Some(("ENXIO\0", "No such device or address\0")),
    Some(("E2BIG\0", "Argument list too long\0")),
    Some(("ENOEXEC\0", "Exec format error\0")),
    Some(("EBADF\0", "Bad file descriptor\0")),
    Some(("ECHILD\0", "No child processes\0")),
    Some(("EAGAIN\0", "Resource temporarily unavailable\0")),
    Some(("ENOMEM\0", "Cannot allocate memory\0")),
    Some(("EACCES\0", "Permission denied\0")),
    Some(("EFAULT\0", "Bad address\0")),
    Some(("ENOTBLK\0", "Block device required\0")),
    Some(("EBUSY\0", "Device or resource busy\0")),
    Some(("EEXIST\0", "File exists\0")),
    Some(("EXDEV\0", "Invalid cross-device link\0")),
    Some(("ENODEV\0", "No such device\0")),
    Some(("ENOTDIR\0", "Not a directory\0")),
    Some(("EISDIR\0", "Is a directory\0")),
    Some(("EINVAL\0", "Invalid argument\0")),
    Some(("ENFILE\0", "Too many open files in system\0")),
    Some(("EMFILE\0", "Too many open files\0")),
    Some(("ENOTTY\0", "Inappropriate ioctl for device\0")),
    Some(("ETXTBSY\0", "Text file busy\0")),
    Some(("EFBIG\0", "File too large\0")),
    Some(("ENOSPC\0", "No space left on device\0")),
    Some(("ESPIPE\0", "Illegal seek\0")),
    Some(("EROFS\0", "Read-only file system\0")),
    Some(("EMLINK\0", "Too many links\0")),
    Some(("EPIPE\0", "Broken pipe\0")),
    Some(("EDOM\0", "Numerical argument out of domain\0")),
    Some(("ERANGE\0", "Numerical result out of range\0")),
    Some(("EDEADLK\0", "Resource deadlock avoided\0")),
    Some(("ENAMETOOLONG\0", "File name too long\0")),
    Some(("ENOLCK\0", "No locks available\0")),
    Some(("ENOSYS\0", "Function not implemented\0")),
    Some(("ENOTEMPTY\0", "Directory not empty\0")),
    Some(("ELOOP\0", "Too many levels of symbolic links\0")),
    None,
    Some(("ENOMSG\0", "No message of desired type\0")),
    Some(("EIDRM\0", "Identifier removed\0")),
    Some(("ECHRNG\0", "Channel number out of range\0")),
    Some(("EL2NSYNC\0", "Level 2 not synchronized\0")),
    Some(("EL3HLT\0", "Level 3 halted\0")),
    Some(("EL3RST\0", "Level 3 reset\0")),
    Some(("ELNRNG\0", "Link number out of range\0")),
    Some(("EUNATCH\0", "Protocol driver not attached\0")),
    Some(("ENOCSI\0", "No CSI structure available\0")),
    Some(("EL2HLT\0", "Level 2 halted\0")),
    Some(("EBADE\0", "Invalid exchange\0")),
    Some(("EBADR\0", "Invalid request descriptor\0")),
    Some(("EXFULL\0", "Exchange full\0")),
    Some(("ENOANO\0", "No anode\0")),
    Some(("EBADRQC\0", "Invalid request code\0")),
    Some(("EBADSLT\0", "Invalid slot\0")),
    None,
    Some(("EBFONT\0", "Bad font file format\0")),
    Some(("ENOSTR\0", "Device not a stream\0")),
    Some(("ENODATA\0", "No data available\0")),
    Some(("ETIME\0", "Timer expired\0")),
    Some(("ENOSR\0", "Out of streams resources\0")),
    Some(("ENONET\0", "Machine is not on the network\0")),
    Some(("ENOPKG\0", "Package not installed\0")),
    Some(("EREMOTE\0", "Object is remote\0")),
    Some(("ENOLINK\0", "Link has been severed\0")),
    Some(("EADV\0", "Advertise error\0")),
    Some(("ESRMNT\0", "Srmount error\0")),
    Some(("ECOMM\0", "Communication error on send\0")),
    Some(("EPROTO\0", "Protocol error\0")),
    Some(("EMULTIHOP\0", "Multihop attempted\0")),
    Some(("EDOTDOT\0", "RFS specific error\0")),
    Some(("EBADMSG\0", "Bad message\0")),
    Some(("EOVERFLOW\0", "Value too large for defined data type\0")),
    Some(("ENOTUNIQ\0", "Name not unique on network\0")),
    Some(("EBADFD\0", "File descriptor in bad state\0")),
    Some(("EREMCHG\0", "Remote address changed\0")),
    Some(("ELIBACC\0", "Can not access a needed shared library\0")),
    Some(("ELIBBAD\0", "Accessing a corrupted shared library\0")),
    Some(("ELIBSCN\0", ".lib section in a.out corrupted\0")),
    Some(("ELIBMAX\0", "Attempting to link in too many shared libraries\0")),
    Some(("ELIBEXEC\0", "Cannot exec a shared library directly\0")),
    Some(("EILSEQ\0", "Invalid or incomplete multibyte or wide character\0")),
    Some(("ERESTART\0", "Interrupted system call should be restarted\0")),
    Some(("ESTRPIPE\0", "Streams pipe error\0")),
    Some(("EUSERS\0", "Too many users\0")),
    Some(("ENOTSOCK\0", "Socket operation on non-socket\0")),
    Some(("EDESTADDRREQ\0", "Destination address required\0")),
    Some(("EMSGSIZE\0", "Message too long\0")),
    Some(("EPROTOTYPE\0", "Protocol wrong type for socket\0")),
    Some(("ENOPROTOOPT\0", "Protocol not available\0")),
    Some(("EPROTONOSUPPORT\0", "Protocol not supported\0")),
    Some(("ESOCKTNOSUPPORT\0", "Socket type not supported\0")),
    Some(("EOPNOTSUPP\0", "Operation not supported\0")),
    Some(("EPFNOSUPPORT\0", "Protocol family not supported\0")),
    Some(("EAFNOSUPPORT\0", "Address family not supported by protocol\0")),
    Some(("EADDRINUSE\0", "Address already in use\0")),
    Some(("EADDRNOTAVAIL\0", "Cannot assign requested address\0")),
    Some(("ENETDOWN\0", "Network is down\0")),
    Some(("ENETUNREACH\0", "Network is unreachable\0")),
    Some(("ENETRESET\0", "Network dropped connection on reset\0")),
    Some(("ECONNABORTED\0", "Software caused connection abort\0")),
    Some(("ECONNRESET\0", "Connection reset by peer\0")),
    Some(("ENOBUFS\0", "No buffer space available\0")),
    Some(("EISCONN\0", "Transport endpoint is already connected\0")),
    Some(("ENOTCONN\0", "Transport endpoint is not connected\0")),
    Some(("ESHUTDOWN\0", "Cannot send after transport endpoint shutdown\0")),
    Some(("ETOOMANYREFS\0", "Too many references: cannot splice\0")),
    Some(("ETIMEDOUT\0", "Connection timed out\0")),
    Some(("ECONNREFUSED\0", "Connection refused\0")),
    Some(("EHOSTDOWN\0", "Host is down\0")),
    Some(("EHOSTUNREACH\0", "No route to host\0")),
    Some(("EALREADY\0", "Operation already in progress\0")),
    Some(("EINPROGRESS\0", "Operation now in progress\0")),
    Some(("ESTALE\0", "Stale file handle\0")),
    Some(("EUCLEAN\0", "Structure needs cleaning\0")),
    Some(("ENOTNAM\0", "Not a XENIX named type file\0")),
    Some(("ENAVAIL\0", "No XENIX semaphores available\0")),
    Some(("EISNAM\0", "Is a named type file\0")),
    Some(("EREMOTEIO\0", "Remote I/O error\0")),
    Some(("EDQUOT\0", "Disk quota exceeded\0")),
    Some(("ENOMEDIUM\0", "No medium found\0")),
    Some(("EMEDIUMTYPE\0", "Wrong medium type\0")),
    Some(("ECANCELED\0", "Operation canceled\0")),
    Some(("ENOKEY\0", "Required key not available\0")),
    Some(("EKEYEXPIRED\0", "Key has expired\0")),
    Some(("EKEYREVOKED\0", "Key has been revoked\0")),
    Some(("EKEYREJECTED\0", "Key was rejected by service\0")),
    Some(("EOWNERDEAD\0", "Owner died\0")),
    Some(("ENOTRECOVERABLE\0", "State not recoverable\0")),
    Some(("ERFKILL\0", "Operation not possible due to RF-kill\0")),
    Some(("EHWPOISON\0", "Memory page has hardware error\0")),
];

fn entry(e: i32) -> Option<(&'static str, &'static str)> {
    usize::try_from(e).ok().and_then(|i| ERRORS.get(i)).copied().flatten()
}

fn text(t: &'static str) -> &'static str {
    t.strip_suffix('\0').unwrap_or(t)
}

/// The description of error number `e` (`strerror` in the C locale), `None` for a number
/// without one (the caller writes `Unknown error N`).
pub fn description(e: i32) -> Option<&'static str> {
    entry(e).map(|(_, d)| text(d))
}

/// The symbolic name of error number `e` (`strerrorname_np`): `"0"` for 0.
pub fn name(e: i32) -> Option<&'static str> {
    entry(e).map(|(n, _)| text(n))
}

/// [`description`] with its terminating NUL (for a C caller).
pub fn description_c(e: i32) -> Option<&'static [u8]> {
    entry(e).map(|(_, d)| d.as_bytes())
}

/// [`name`] with its terminating NUL (for a C caller).
pub fn name_c(e: i32) -> Option<&'static [u8]> {
    entry(e).map(|(n, _)| n.as_bytes())
}

/// The text for an error number without a description: `Unknown error N`, into `buf`; the
/// length.
pub fn unknown(e: i32, buf: &mut [u8; 32]) -> usize {
    const HEAD: &[u8] = b"Unknown error ";
    let mut n = 0usize;
    for &b in HEAD {
        if let Some(slot) = buf.get_mut(n) {
            *slot = b;
            n = n.saturating_add(1);
        }
    }
    if e < 0 {
        if let Some(slot) = buf.get_mut(n) {
            *slot = b'-';
            n = n.saturating_add(1);
        }
    }
    let mut digits = [0u8; 10];
    let mut d = 0usize;
    let mut v = e.unsigned_abs();
    loop {
        if let Some(slot) = digits.get_mut(d) {
            *slot = b'0'.saturating_add(u8::try_from(v % 10).unwrap_or(0));
        }
        d = d.saturating_add(1);
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for &b in digits.get(..d).unwrap_or(&[]).iter().rev() {
        if let Some(slot) = buf.get_mut(n) {
            *slot = b;
            n = n.saturating_add(1);
        }
    }
    n
}

/// One frame's text as `backtrace_symbols` and `backtrace_symbols_fd` give it (spec §2.7;
/// D-2.7): the object's name, then `(+0xOFFSET) [0xADDRESS]`, where the offset is the address
/// less the object's load bias (its address in the object's own file layout); or only
/// `[0xADDRESS]` for an address in no known object. No symbol name is ever shown: names are
/// available only for dynamic symbols, and a frame's object is a static executable or carries
/// no symbol table the library reads. The `_fd` form has no space before `[` and ends with a
/// newline. Hexadecimal digits are lower case, without leading zeros.
#[derive(Clone, Copy, Debug)]
pub struct FrameText<'a> {
    name: &'a [u8],
    tail: [u8; 48],
    len: usize,
}

impl<'a> FrameText<'a> {
    /// The text for `address`, inside the object `object` (its name and the address's offset
    /// from its load bias) or in none; `fd` selects the `backtrace_symbols_fd` form.
    pub fn new(address: usize, object: Option<(&'a [u8], usize)>, fd: bool) -> Self {
        let mut t = FrameText { name: &[], tail: [0; 48], len: 0 };
        if let Some((name, offset)) = object {
            t.name = name;
            t.put(b"(+");
            t.hex(offset);
            t.put(if fd { b")" } else { b") " });
        }
        t.put(b"[");
        t.hex(address);
        t.put(if fd { b"]\n" } else { b"]" });
        t
    }

    /// The text in two parts: the object's name and the rest.
    pub fn parts(&self) -> [&[u8]; 2] {
        [self.name, self.tail.get(..self.len).unwrap_or(&[])]
    }

    /// The length of the whole text.
    pub fn len(&self) -> usize {
        self.name.len().saturating_add(self.len)
    }

    /// Whether the text is empty (it never is).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn put(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if let Some(slot) = self.tail.get_mut(self.len) {
                *slot = b;
                self.len = self.len.saturating_add(1);
            }
        }
    }

    fn hex(&mut self, v: usize) {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        self.put(b"0x");
        let mut started = false;
        for shift in (0..usize::BITS).step_by(4).rev() {
            let d = v.checked_shr(shift).unwrap_or(0) & 0xf;
            if d != 0 || started || shift == 0 {
                started = true;
                self.put(DIGITS.get(d..=d).unwrap_or(b"0"));
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod frame_tests {
    use super::FrameText;

    fn whole(t: &FrameText<'_>) -> Vec<u8> {
        t.parts().concat()
    }

    #[test]
    fn frame_texts() {
        assert_eq!(whole(&FrameText::new(0x40_1234, Some((b"./prog", 0x40_1234)), false)), b"./prog(+0x401234) [0x401234]");
        assert_eq!(whole(&FrameText::new(0x5555_0000_122c, Some((b"/a/b", 0x122c)), true)), b"/a/b(+0x122c)[0x55550000122c]\n");
        assert_eq!(whole(&FrameText::new(0, None, false)), b"[0x0]");
        assert_eq!(whole(&FrameText::new(usize::MAX, None, true)), b"[0xffffffffffffffff]\n");
        assert_eq!(FrameText::new(1, Some((b"", 0)), false).len(), b"(+0x0) [0x1]".len());
    }
}
