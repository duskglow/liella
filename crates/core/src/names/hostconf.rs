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

/// The variables whose value is neither `on` nor `off`, as [`BAD_MULTI`] and [`BAD_REORDER`].
pub const BAD_MULTI: u8 = 1;
/// See [`BAD_MULTI`].
pub const BAD_REORDER: u8 = 2;

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

    /// Which variables hold a value that is neither `on` nor `off` ([`BAD_MULTI`], [`BAD_REORDER`]).
    pub fn bad(&self) -> u8 {
        let m = if self.multi == Some(None) { BAD_MULTI } else { 0 };
        let r = if self.reorder == Some(None) { BAD_REORDER } else { 0 };
        m | r
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

/// `on` or `off` at the start of `v` (ASCII case ignored, what follows is not looked at, as the
/// reference does); `None` for anything else.
pub fn on_off(v: &[u8]) -> Option<bool> {
    on_off_rest(v).map(|(on, _)| on)
}

/// [`on_off`] with the text after the word.
pub fn on_off_rest(v: &[u8]) -> Option<(bool, &[u8])> {
    if let Some((h, t)) = v.split_at_checked(2)
        && h.eq_ignore_ascii_case(b"on")
    {
        Some((true, t))
    } else if let Some((h, t)) = v.split_at_checked(3)
        && h.eq_ignore_ascii_case(b"off")
    {
        Some((false, t))
    } else {
        None
    }
}

fn is_space(c: u8) -> bool {
    c.is_ascii_whitespace() || c == 0x0b
}

fn skip_space(s: &[u8]) -> &[u8] {
    let n = s.iter().position(|&c| !is_space(c)).unwrap_or(s.len());
    s.get(n..).unwrap_or(&[])
}

/// A problem the reference reports on stderr (the text it quotes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem<'a> {
    /// ``bad command `LINE'``.
    BadCommand(&'a [u8]),
    /// ``expected `on' or `off', found `VALUE'``.
    Expected(&'a [u8]),
    /// ``ignoring trailing garbage `TEXT'``.
    Garbage(&'a [u8]),
}

impl Problem<'_> {
    /// The message as pieces (concatenated: the text up to and including the closing quote).
    pub fn pieces(&self) -> [&[u8]; 3] {
        match *self {
            Problem::BadCommand(l) => [b"bad command `", l, b"'"],
            Problem::Expected(v) => [b"expected `on' or `off', found `", v, b"'"],
            Problem::Garbage(g) => [b"ignoring trailing garbage `", g, b"'"],
        }
    }
}

/// What one `host.conf` line sets.
enum Item<'a> {
    Multi(bool),
    Reorder(bool),
    Trim(&'a [u8]),
    Problem(Problem<'a>),
}

/// The lines of a `host.conf` text: blank and `#` lines are skipped, keywords match with ASCII case
/// ignored (`order` is accepted and ignored); `f` gets the 1-based line number and what the line says.
fn scan<'a>(text: &'a [u8], mut f: impl FnMut(usize, Item<'a>)) {
    for (i, raw) in text.split(|&c| c == b'\n').enumerate() {
        let line = skip_space(raw);
        if line.is_empty() || line.first() == Some(&b'#') {
            continue;
        }
        let n = i.saturating_add(1);
        let kw_end = line.iter().position(|&c| is_space(c)).unwrap_or(line.len());
        let (key, rest) = line.split_at(kw_end);
        let rest = skip_space(rest);
        let is_multi = key.eq_ignore_ascii_case(b"multi");
        if is_multi || key.eq_ignore_ascii_case(b"reorder") {
            match on_off_rest(rest) {
                Some((v, after)) => {
                    f(n, if is_multi { Item::Multi(v) } else { Item::Reorder(v) });
                    let a = skip_space(after);
                    if !a.is_empty() && a.first() != Some(&b'#') {
                        f(n, Item::Problem(Problem::Garbage(a)));
                    }
                }
                None => f(n, Item::Problem(Problem::Expected(rest))),
            }
        } else if key.eq_ignore_ascii_case(b"trim") {
            f(n, Item::Trim(rest));
        } else if !key.eq_ignore_ascii_case(b"order") {
            f(n, Item::Problem(Problem::BadCommand(line)));
        }
    }
}

/// The settings of a `host.conf` text (`None`: no file) with the environment `env` applied.
pub fn parse(text: Option<&[u8]>, env: &Env) -> Settings {
    let mut s = Settings::default();
    scan(text.unwrap_or(&[]), |_, item| match item {
        Item::Multi(v) => s.multi = v,
        Item::Reorder(v) => s.reorder = v,
        Item::Trim(rest) => {
            for d in words(rest) {
                s.trim.add_list(d);
            }
        }
        Item::Problem(_) => {}
    });
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

/// The problems in a `host.conf` text, with their 1-based line numbers, in order.
pub fn problems<'a>(text: &'a [u8], mut f: impl FnMut(usize, Problem<'a>)) {
    scan(text, |n, item| {
        if let Item::Problem(p) = item {
            f(n, p);
        }
    });
}

/// Whether a `host.conf` text has a problem (stops at no line: one scan).
pub fn has_problem(text: &[u8]) -> bool {
    let mut any = false;
    problems(text, |_, _| any = true);
    any
}

fn words(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    let end = line.iter().position(|&c| c == b'#').unwrap_or(line.len());
    line.get(..end).unwrap_or(line).split(|&c| is_space(c)).filter(|w| !w.is_empty())
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
