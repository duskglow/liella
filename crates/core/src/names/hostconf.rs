//! `host.conf` (§16.3, D-16.13): the keywords and `RESOLV_*` overrides the reference honours,
//! settled by black-box probe (`crates/core/tests/hostconf_probe.rs` holds the record).
//!
//! * `multi on|off`: every address of a name in `hosts`, not only the first line's.
//! * `reorder on|off`: a by-name answer's first address on the network of a local interface
//!   (its own prefix length; loopback counts) swaps places with the answer's first address.
//! * `trim <domain>...`: a by-address answer's names lose a domain they end in (ASCII case
//!   ignored, the name strictly longer); up to [`MAX_TRIM`] domains, separated by white space,
//!   `,` or `;`.
//! * Keywords and `on`/`off` ignore ASCII case; a value other than `on`/`off` leaves the
//!   setting as it was; the last line wins; `#` starts a comment.
//! * `RESOLV_MULTI` and `RESOLV_REORDER` override the file; `RESOLV_OVERRIDE_TRIM_DOMAINS`
//!   replaces its domains and `RESOLV_ADD_TRIM_DOMAINS` appends to them (one domain list each,
//!   same separators); `RESOLV_HOST_CONF` names another file.
//! * Ignored as the reference ignores them: `order`, `spoof`, `nospoof`, `spoofalert` and every
//!   other keyword, `RESOLV_SPOOF_CHECK` and `RESOLV_SERV_ORDER`.
//!
//! The environment is read once by the caller ([`Env`]) and applied at every load of the file,
//! so a lookup never walks the environment.

/// The most trim domains (the reference warns and ignores the rest).
pub const MAX_TRIM: usize = 4;
/// The longest trim domain kept (a longer one is ignored).
pub const MAX_DOMAIN: usize = 127;

/// A list of up to [`MAX_TRIM`] domains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Domains {
    n: u8,
    lens: [u8; MAX_TRIM],
    text: [[u8; MAX_DOMAIN]; MAX_TRIM],
}

impl Default for Domains {
    fn default() -> Domains {
        Domains::EMPTY
    }
}

impl Domains {
    /// No domain.
    pub const EMPTY: Domains = Domains { n: 0, lens: [0; MAX_TRIM], text: [[0; MAX_DOMAIN]; MAX_TRIM] };

    /// The domains, in order.
    pub fn iter(&self) -> impl Iterator<Item = &[u8]> {
        self.text.iter().zip(self.lens).take(usize::from(self.n)).map(|(t, l)| t.get(..usize::from(l)).unwrap_or(&[]))
    }

    /// How many.
    pub fn len(&self) -> usize {
        usize::from(self.n)
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Append the domains of a list text (white space, `,` or `;` between them).
    pub fn add_list(&mut self, list: &[u8]) {
        for d in list.split(|c| matches!(c, b' ' | b'\t' | b',' | b';')).filter(|d| !d.is_empty()) {
            let i = usize::from(self.n);
            if i >= MAX_TRIM {
                return;
            }
            let (Some(slot), Some(len)) = (self.text.get_mut(i), self.lens.get_mut(i)) else { return };
            if let (Some(dst), Ok(l)) = (slot.get_mut(..d.len()), u8::try_from(d.len())) {
                dst.copy_from_slice(d);
                *len = l;
                self.n = self.n.saturating_add(1);
            }
        }
    }
}

/// The `RESOLV_*` values, read once (`None`: unset).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Env {
    /// `RESOLV_MULTI`: `Some(Some(on))`, or `Some(None)` for a value that is neither.
    pub multi: Option<Option<bool>>,
    /// `RESOLV_REORDER`, as `multi`.
    pub reorder: Option<Option<bool>>,
    /// `RESOLV_OVERRIDE_TRIM_DOMAINS`.
    pub override_trim: Option<Domains>,
    /// `RESOLV_ADD_TRIM_DOMAINS`.
    pub add_trim: Option<Domains>,
}

impl Env {
    /// The values from a lookup of each variable (called once per variable name).
    pub fn read<'a>(mut get: impl FnMut(&[u8]) -> Option<&'a [u8]>) -> Env {
        let list = |v: &[u8]| {
            let mut d = Domains::EMPTY;
            d.add_list(v);
            d
        };
        Env {
            multi: get(b"RESOLV_MULTI").map(on_off),
            reorder: get(b"RESOLV_REORDER").map(on_off),
            override_trim: get(b"RESOLV_OVERRIDE_TRIM_DOMAINS").map(list),
            add_trim: get(b"RESOLV_ADD_TRIM_DOMAINS").map(list),
        }
    }
}

/// The effective settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// `multi on`.
    pub multi: bool,
    /// `reorder on`.
    pub reorder: bool,
    /// The trim domains.
    pub trim: Domains,
}

/// `on` or `off` (ASCII case ignored); `None` for anything else.
pub fn on_off(v: &[u8]) -> Option<bool> {
    if v.eq_ignore_ascii_case(b"on") {
        Some(true)
    } else if v.eq_ignore_ascii_case(b"off") {
        Some(false)
    } else {
        None
    }
}

fn words(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    let end = line.iter().position(|&c| c == b'#').unwrap_or(line.len());
    line.get(..end).unwrap_or(line).split(|c| c.is_ascii_whitespace()).filter(|w| !w.is_empty())
}

/// The settings of a `host.conf` text (`None`: no file) with the environment `env` applied.
pub fn parse(text: Option<&[u8]>, env: &Env) -> Settings {
    let mut s = Settings::default();
    for line in text.unwrap_or(&[]).split(|&c| c == b'\n') {
        let mut w = words(line);
        let Some(key) = w.next() else { continue };
        if key.eq_ignore_ascii_case(b"multi") || key.eq_ignore_ascii_case(b"reorder") {
            let slot = if key.eq_ignore_ascii_case(b"multi") { &mut s.multi } else { &mut s.reorder };
            if let Some(v) = w.next().and_then(on_off) {
                *slot = v;
            }
        } else if key.eq_ignore_ascii_case(b"trim") {
            for d in w {
                s.trim.add_list(d);
            }
        }
    }
    if let Some(Some(v)) = env.multi {
        s.multi = v;
    }
    if let Some(Some(v)) = env.reorder {
        s.reorder = v;
    }
    if let Some(d) = env.override_trim {
        s.trim = d;
    }
    if let Some(d) = env.add_trim {
        for x in d.iter() {
            s.trim.add_list(x);
        }
    }
    s
}

/// `name` without the first trim domain it ends in (ASCII case ignored, `name` longer).
pub fn trim<'a>(name: &'a [u8], domains: &Domains) -> &'a [u8] {
    for d in domains.iter() {
        let cut = name.len().checked_sub(d.len()).filter(|&k| k > 0).and_then(|k| name.split_at_checked(k));
        if let Some((head, tail)) = cut
            && tail.eq_ignore_ascii_case(d)
        {
            return head;
        }
    }
    name
}

/// The index of the first of `addrs` on the network of one of `nets` (address, prefix
/// length), when it is not the first: `reorder` swaps it with index 0.
pub fn reorder_index(addrs: &[[u8; 4]], nets: &[([u8; 4], u8)]) -> Option<usize> {
    let on_net = |a: &[u8; 4]| {
        nets.iter().any(|(n, p)| {
            let mask = u32::MAX.checked_shl(32u32.saturating_sub(u32::from(*p))).unwrap_or(0);
            (u32::from_be_bytes(*a) ^ u32::from_be_bytes(*n)) & mask == 0
        })
    };
    addrs.iter().position(on_net).filter(|&i| i > 0)
}
