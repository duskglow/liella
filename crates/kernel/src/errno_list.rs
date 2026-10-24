// @generated from the Linux UAPI headers asm-generic/errno-base.h and asm-generic/errno.h
// (normative kernel ABI, OD-2). Regenerate: see crates/kernel/src/errno.rs.

use super::Errno;

impl Errno {
    /// Operation not permitted (1).
    pub const EPERM: Errno = Errno::lit(1);
    /// No such file or directory (2).
    pub const ENOENT: Errno = Errno::lit(2);
    /// No such process (3).
    pub const ESRCH: Errno = Errno::lit(3);
    /// Interrupted system call (4).
    pub const EINTR: Errno = Errno::lit(4);
    /// I/O error (5).
    pub const EIO: Errno = Errno::lit(5);
    /// No such device or address (6).
    pub const ENXIO: Errno = Errno::lit(6);
    /// Argument list too long (7).
    pub const E2BIG: Errno = Errno::lit(7);
    /// Exec format error (8).
    pub const ENOEXEC: Errno = Errno::lit(8);
    /// Bad file number (9).
    pub const EBADF: Errno = Errno::lit(9);
    /// No child processes (10).
    pub const ECHILD: Errno = Errno::lit(10);
    /// Try again (11).
    pub const EAGAIN: Errno = Errno::lit(11);
    /// Out of memory (12).
    pub const ENOMEM: Errno = Errno::lit(12);
    /// Permission denied (13).
    pub const EACCES: Errno = Errno::lit(13);
    /// Bad address (14).
    pub const EFAULT: Errno = Errno::lit(14);
    /// Block device required (15).
    pub const ENOTBLK: Errno = Errno::lit(15);
    /// Device or resource busy (16).
    pub const EBUSY: Errno = Errno::lit(16);
    /// File exists (17).
    pub const EEXIST: Errno = Errno::lit(17);
    /// Cross-device link (18).
    pub const EXDEV: Errno = Errno::lit(18);
    /// No such device (19).
    pub const ENODEV: Errno = Errno::lit(19);
    /// Not a directory (20).
    pub const ENOTDIR: Errno = Errno::lit(20);
    /// Is a directory (21).
    pub const EISDIR: Errno = Errno::lit(21);
    /// Invalid argument (22).
    pub const EINVAL: Errno = Errno::lit(22);
    /// File table overflow (23).
    pub const ENFILE: Errno = Errno::lit(23);
    /// Too many open files (24).
    pub const EMFILE: Errno = Errno::lit(24);
    /// Not a typewriter (25).
    pub const ENOTTY: Errno = Errno::lit(25);
    /// Text file busy (26).
    pub const ETXTBSY: Errno = Errno::lit(26);
    /// File too large (27).
    pub const EFBIG: Errno = Errno::lit(27);
    /// No space left on device (28).
    pub const ENOSPC: Errno = Errno::lit(28);
    /// Illegal seek (29).
    pub const ESPIPE: Errno = Errno::lit(29);
    /// Read-only file system (30).
    pub const EROFS: Errno = Errno::lit(30);
    /// Too many links (31).
    pub const EMLINK: Errno = Errno::lit(31);
    /// Broken pipe (32).
    pub const EPIPE: Errno = Errno::lit(32);
    /// Math argument out of domain of func (33).
    pub const EDOM: Errno = Errno::lit(33);
    /// Math result not representable (34).
    pub const ERANGE: Errno = Errno::lit(34);
    /// Resource deadlock would occur (35).
    pub const EDEADLK: Errno = Errno::lit(35);
    /// File name too long (36).
    pub const ENAMETOOLONG: Errno = Errno::lit(36);
    /// No record locks available (37).
    pub const ENOLCK: Errno = Errno::lit(37);
    /// Invalid system call number (38).
    pub const ENOSYS: Errno = Errno::lit(38);
    /// Directory not empty (39).
    pub const ENOTEMPTY: Errno = Errno::lit(39);
    /// Too many symbolic links encountered (40).
    pub const ELOOP: Errno = Errno::lit(40);
    /// No message of desired type (42).
    pub const ENOMSG: Errno = Errno::lit(42);
    /// Identifier removed (43).
    pub const EIDRM: Errno = Errno::lit(43);
    /// Channel number out of range (44).
    pub const ECHRNG: Errno = Errno::lit(44);
    /// Level 2 not synchronized (45).
    pub const EL2NSYNC: Errno = Errno::lit(45);
    /// Level 3 halted (46).
    pub const EL3HLT: Errno = Errno::lit(46);
    /// Level 3 reset (47).
    pub const EL3RST: Errno = Errno::lit(47);
    /// Link number out of range (48).
    pub const ELNRNG: Errno = Errno::lit(48);
    /// Protocol driver not attached (49).
    pub const EUNATCH: Errno = Errno::lit(49);
    /// No CSI structure available (50).
    pub const ENOCSI: Errno = Errno::lit(50);
    /// Level 2 halted (51).
    pub const EL2HLT: Errno = Errno::lit(51);
    /// Invalid exchange (52).
    pub const EBADE: Errno = Errno::lit(52);
    /// Invalid request descriptor (53).
    pub const EBADR: Errno = Errno::lit(53);
    /// Exchange full (54).
    pub const EXFULL: Errno = Errno::lit(54);
    /// No anode (55).
    pub const ENOANO: Errno = Errno::lit(55);
    /// Invalid request code (56).
    pub const EBADRQC: Errno = Errno::lit(56);
    /// Invalid slot (57).
    pub const EBADSLT: Errno = Errno::lit(57);
    /// Bad font file format (59).
    pub const EBFONT: Errno = Errno::lit(59);
    /// Device not a stream (60).
    pub const ENOSTR: Errno = Errno::lit(60);
    /// No data available (61).
    pub const ENODATA: Errno = Errno::lit(61);
    /// Timer expired (62).
    pub const ETIME: Errno = Errno::lit(62);
    /// Out of streams resources (63).
    pub const ENOSR: Errno = Errno::lit(63);
    /// Machine is not on the network (64).
    pub const ENONET: Errno = Errno::lit(64);
    /// Package not installed (65).
    pub const ENOPKG: Errno = Errno::lit(65);
    /// Object is remote (66).
    pub const EREMOTE: Errno = Errno::lit(66);
    /// Link has been severed (67).
    pub const ENOLINK: Errno = Errno::lit(67);
    /// Advertise error (68).
    pub const EADV: Errno = Errno::lit(68);
    /// Srmount error (69).
    pub const ESRMNT: Errno = Errno::lit(69);
    /// Communication error on send (70).
    pub const ECOMM: Errno = Errno::lit(70);
    /// Protocol error (71).
    pub const EPROTO: Errno = Errno::lit(71);
    /// Multihop attempted (72).
    pub const EMULTIHOP: Errno = Errno::lit(72);
    /// RFS specific error (73).
    pub const EDOTDOT: Errno = Errno::lit(73);
    /// Not a data message (74).
    pub const EBADMSG: Errno = Errno::lit(74);
    /// Value too large for defined data type (75).
    pub const EOVERFLOW: Errno = Errno::lit(75);
    /// Name not unique on network (76).
    pub const ENOTUNIQ: Errno = Errno::lit(76);
    /// File descriptor in bad state (77).
    pub const EBADFD: Errno = Errno::lit(77);
    /// Remote address changed (78).
    pub const EREMCHG: Errno = Errno::lit(78);
    /// Can not access a needed shared library (79).
    pub const ELIBACC: Errno = Errno::lit(79);
    /// Accessing a corrupted shared library (80).
    pub const ELIBBAD: Errno = Errno::lit(80);
    /// .lib section in a.out corrupted (81).
    pub const ELIBSCN: Errno = Errno::lit(81);
    /// Attempting to link in too many shared libraries (82).
    pub const ELIBMAX: Errno = Errno::lit(82);
    /// Cannot exec a shared library directly (83).
    pub const ELIBEXEC: Errno = Errno::lit(83);
    /// Illegal byte sequence (84).
    pub const EILSEQ: Errno = Errno::lit(84);
    /// Interrupted system call should be restarted (85).
    pub const ERESTART: Errno = Errno::lit(85);
    /// Streams pipe error (86).
    pub const ESTRPIPE: Errno = Errno::lit(86);
    /// Too many users (87).
    pub const EUSERS: Errno = Errno::lit(87);
    /// Socket operation on non-socket (88).
    pub const ENOTSOCK: Errno = Errno::lit(88);
    /// Destination address required (89).
    pub const EDESTADDRREQ: Errno = Errno::lit(89);
    /// Message too long (90).
    pub const EMSGSIZE: Errno = Errno::lit(90);
    /// Protocol wrong type for socket (91).
    pub const EPROTOTYPE: Errno = Errno::lit(91);
    /// Protocol not available (92).
    pub const ENOPROTOOPT: Errno = Errno::lit(92);
    /// Protocol not supported (93).
    pub const EPROTONOSUPPORT: Errno = Errno::lit(93);
    /// Socket type not supported (94).
    pub const ESOCKTNOSUPPORT: Errno = Errno::lit(94);
    /// Operation not supported on transport endpoint (95).
    pub const EOPNOTSUPP: Errno = Errno::lit(95);
    /// Protocol family not supported (96).
    pub const EPFNOSUPPORT: Errno = Errno::lit(96);
    /// Address family not supported by protocol (97).
    pub const EAFNOSUPPORT: Errno = Errno::lit(97);
    /// Address already in use (98).
    pub const EADDRINUSE: Errno = Errno::lit(98);
    /// Cannot assign requested address (99).
    pub const EADDRNOTAVAIL: Errno = Errno::lit(99);
    /// Network is down (100).
    pub const ENETDOWN: Errno = Errno::lit(100);
    /// Network is unreachable (101).
    pub const ENETUNREACH: Errno = Errno::lit(101);
    /// Network dropped connection because of reset (102).
    pub const ENETRESET: Errno = Errno::lit(102);
    /// Software caused connection abort (103).
    pub const ECONNABORTED: Errno = Errno::lit(103);
    /// Connection reset by peer (104).
    pub const ECONNRESET: Errno = Errno::lit(104);
    /// No buffer space available (105).
    pub const ENOBUFS: Errno = Errno::lit(105);
    /// Transport endpoint is already connected (106).
    pub const EISCONN: Errno = Errno::lit(106);
    /// Transport endpoint is not connected (107).
    pub const ENOTCONN: Errno = Errno::lit(107);
    /// Cannot send after transport endpoint shutdown (108).
    pub const ESHUTDOWN: Errno = Errno::lit(108);
    /// Too many references: cannot splice (109).
    pub const ETOOMANYREFS: Errno = Errno::lit(109);
    /// Connection timed out (110).
    pub const ETIMEDOUT: Errno = Errno::lit(110);
    /// Connection refused (111).
    pub const ECONNREFUSED: Errno = Errno::lit(111);
    /// Host is down (112).
    pub const EHOSTDOWN: Errno = Errno::lit(112);
    /// No route to host (113).
    pub const EHOSTUNREACH: Errno = Errno::lit(113);
    /// Operation already in progress (114).
    pub const EALREADY: Errno = Errno::lit(114);
    /// Operation now in progress (115).
    pub const EINPROGRESS: Errno = Errno::lit(115);
    /// Stale file handle (116).
    pub const ESTALE: Errno = Errno::lit(116);
    /// Structure needs cleaning (117).
    pub const EUCLEAN: Errno = Errno::lit(117);
    /// Not a XENIX named type file (118).
    pub const ENOTNAM: Errno = Errno::lit(118);
    /// No XENIX semaphores available (119).
    pub const ENAVAIL: Errno = Errno::lit(119);
    /// Is a named type file (120).
    pub const EISNAM: Errno = Errno::lit(120);
    /// Remote I/O error (121).
    pub const EREMOTEIO: Errno = Errno::lit(121);
    /// Quota exceeded (122).
    pub const EDQUOT: Errno = Errno::lit(122);
    /// No medium found (123).
    pub const ENOMEDIUM: Errno = Errno::lit(123);
    /// Wrong medium type (124).
    pub const EMEDIUMTYPE: Errno = Errno::lit(124);
    /// Operation Canceled (125).
    pub const ECANCELED: Errno = Errno::lit(125);
    /// Required key not available (126).
    pub const ENOKEY: Errno = Errno::lit(126);
    /// Key has expired (127).
    pub const EKEYEXPIRED: Errno = Errno::lit(127);
    /// Key has been revoked (128).
    pub const EKEYREVOKED: Errno = Errno::lit(128);
    /// Key was rejected by service (129).
    pub const EKEYREJECTED: Errno = Errno::lit(129);
    /// Owner died (130).
    pub const EOWNERDEAD: Errno = Errno::lit(130);
    /// State not recoverable (131).
    pub const ENOTRECOVERABLE: Errno = Errno::lit(131);
    /// Operation not possible due to RF-kill (132).
    pub const ERFKILL: Errno = Errno::lit(132);
    /// Memory page has hardware error (133).
    pub const EHWPOISON: Errno = Errno::lit(133);
    /// Alias of [`Errno::EDEADLK`].
    pub const EDEADLOCK: Errno = Errno::EDEADLK;
    /// Alias of [`Errno::EAGAIN`].
    pub const EWOULDBLOCK: Errno = Errno::EAGAIN;
}

/// `(number, symbolic name)` for every kernel error number, in ascending order.
pub(crate) const NAMES: &[(u16, &str)] = &[
    (1, "EPERM"),
    (2, "ENOENT"),
    (3, "ESRCH"),
    (4, "EINTR"),
    (5, "EIO"),
    (6, "ENXIO"),
    (7, "E2BIG"),
    (8, "ENOEXEC"),
    (9, "EBADF"),
    (10, "ECHILD"),
    (11, "EAGAIN"),
    (12, "ENOMEM"),
    (13, "EACCES"),
    (14, "EFAULT"),
    (15, "ENOTBLK"),
    (16, "EBUSY"),
    (17, "EEXIST"),
    (18, "EXDEV"),
    (19, "ENODEV"),
    (20, "ENOTDIR"),
    (21, "EISDIR"),
    (22, "EINVAL"),
    (23, "ENFILE"),
    (24, "EMFILE"),
    (25, "ENOTTY"),
    (26, "ETXTBSY"),
    (27, "EFBIG"),
    (28, "ENOSPC"),
    (29, "ESPIPE"),
    (30, "EROFS"),
    (31, "EMLINK"),
    (32, "EPIPE"),
    (33, "EDOM"),
    (34, "ERANGE"),
    (35, "EDEADLK"),
    (36, "ENAMETOOLONG"),
    (37, "ENOLCK"),
    (38, "ENOSYS"),
    (39, "ENOTEMPTY"),
    (40, "ELOOP"),
    (42, "ENOMSG"),
    (43, "EIDRM"),
    (44, "ECHRNG"),
    (45, "EL2NSYNC"),
    (46, "EL3HLT"),
    (47, "EL3RST"),
    (48, "ELNRNG"),
    (49, "EUNATCH"),
    (50, "ENOCSI"),
    (51, "EL2HLT"),
    (52, "EBADE"),
    (53, "EBADR"),
    (54, "EXFULL"),
    (55, "ENOANO"),
    (56, "EBADRQC"),
    (57, "EBADSLT"),
    (59, "EBFONT"),
    (60, "ENOSTR"),
    (61, "ENODATA"),
    (62, "ETIME"),
    (63, "ENOSR"),
    (64, "ENONET"),
    (65, "ENOPKG"),
    (66, "EREMOTE"),
    (67, "ENOLINK"),
    (68, "EADV"),
    (69, "ESRMNT"),
    (70, "ECOMM"),
    (71, "EPROTO"),
    (72, "EMULTIHOP"),
    (73, "EDOTDOT"),
    (74, "EBADMSG"),
    (75, "EOVERFLOW"),
    (76, "ENOTUNIQ"),
    (77, "EBADFD"),
    (78, "EREMCHG"),
    (79, "ELIBACC"),
    (80, "ELIBBAD"),
    (81, "ELIBSCN"),
    (82, "ELIBMAX"),
    (83, "ELIBEXEC"),
    (84, "EILSEQ"),
    (85, "ERESTART"),
    (86, "ESTRPIPE"),
    (87, "EUSERS"),
    (88, "ENOTSOCK"),
    (89, "EDESTADDRREQ"),
    (90, "EMSGSIZE"),
    (91, "EPROTOTYPE"),
    (92, "ENOPROTOOPT"),
    (93, "EPROTONOSUPPORT"),
    (94, "ESOCKTNOSUPPORT"),
    (95, "EOPNOTSUPP"),
    (96, "EPFNOSUPPORT"),
    (97, "EAFNOSUPPORT"),
    (98, "EADDRINUSE"),
    (99, "EADDRNOTAVAIL"),
    (100, "ENETDOWN"),
    (101, "ENETUNREACH"),
    (102, "ENETRESET"),
    (103, "ECONNABORTED"),
    (104, "ECONNRESET"),
    (105, "ENOBUFS"),
    (106, "EISCONN"),
    (107, "ENOTCONN"),
    (108, "ESHUTDOWN"),
    (109, "ETOOMANYREFS"),
    (110, "ETIMEDOUT"),
    (111, "ECONNREFUSED"),
    (112, "EHOSTDOWN"),
    (113, "EHOSTUNREACH"),
    (114, "EALREADY"),
    (115, "EINPROGRESS"),
    (116, "ESTALE"),
    (117, "EUCLEAN"),
    (118, "ENOTNAM"),
    (119, "ENAVAIL"),
    (120, "EISNAM"),
    (121, "EREMOTEIO"),
    (122, "EDQUOT"),
    (123, "ENOMEDIUM"),
    (124, "EMEDIUMTYPE"),
    (125, "ECANCELED"),
    (126, "ENOKEY"),
    (127, "EKEYEXPIRED"),
    (128, "EKEYREVOKED"),
    (129, "EKEYREJECTED"),
    (130, "EOWNERDEAD"),
    (131, "ENOTRECOVERABLE"),
    (132, "ERFKILL"),
    (133, "EHWPOISON"),
];

/// The largest kernel error number on this ABI.
pub const MAX_ERRNO: u16 = 133;
