//! The texts of the name-service error codes (spec §16.2 `gai_strerror`, §16.3 `hstrerror`,
//! `herror`, `h_errlist`), as the reference gives them in the "C" locale (netdb.md, dns.md).
//! Each is NUL-terminated so that the C functions can return it directly.

/// `gai_strerror(code)`: the text of an `EAI_*` code (`Unknown error` for any other value).
pub fn gai(code: i32) -> &'static [u8] {
    match code {
        0 => b"Success\0",
        -1 => b"Bad value for ai_flags\0",
        -2 => b"Name or service not known\0",
        -3 => b"Temporary failure in name resolution\0",
        -4 => b"Non-recoverable failure in name resolution\0",
        -5 => b"No address associated with hostname\0",
        -6 => b"ai_family not supported\0",
        -7 => b"ai_socktype not supported\0",
        -8 => b"Servname not supported for ai_socktype\0",
        -9 => b"Address family for hostname not supported\0",
        -10 => b"Memory allocation failure\0",
        -11 => b"System error\0",
        -12 => b"Result too large for supplied buffer\0",
        -100 => b"Processing request in progress\0",
        -101 => b"Request canceled\0",
        -102 => b"Request not canceled\0",
        -103 => b"All requests done\0",
        -104 => b"Interrupted by a signal\0",
        -105 => b"Parameter string not correctly encoded\0",
        _ => b"Unknown error\0",
    }
}

/// The texts of `h_errno` 0 to 4 (`h_errlist`).
pub const H_ERRLIST: [&[u8]; 5] = [
    b"Resolver Error 0 (no error)\0",
    b"Unknown host\0",
    b"Host name lookup failure\0",
    b"Unknown server error\0",
    b"No address associated with name\0",
];

/// `hstrerror(code)`: the text of an `h_errno` value (`NETDB_INTERNAL`, -1, and every other
/// negative value is the internal error; values beyond the list the unknown one).
pub fn h_error(code: i32) -> &'static [u8] {
    if code < 0 {
        return b"Resolver internal error\0";
    }
    usize::try_from(code).ok().and_then(|i| H_ERRLIST.get(i)).copied().unwrap_or(b"Unknown resolver error\0")
}

#[cfg(test)]
mod tests {
    #[test]
    fn texts() {
        assert_eq!(super::gai(-8), b"Servname not supported for ai_socktype\0");
        assert_eq!(super::gai(1), b"Unknown error\0");
        assert_eq!(super::h_error(-1), b"Resolver internal error\0");
        assert_eq!(super::h_error(5), b"Unknown resolver error\0");
        assert_eq!(super::h_error(1), b"Unknown host\0");
    }
}
