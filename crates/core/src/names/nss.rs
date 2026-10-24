//! The Name Service Switch (spec §16.1; D-16.1, D-16.2) as values: what a database is looked up
//! in, in which order, and what each outcome does.
//!
//! **Configuration** (`nsswitch.conf(5)`). One line per database: its name, a colon, then the
//! sources in order, each optionally followed by `[STATUS=ACTION ...]` rules for the outcome of
//! that source. `#` starts a comment. Database and source names are case-sensitive (they name
//! modules; `passwd: Files` names no source, as the reference reads it); the status and action
//! keywords are not. A line naming an unknown database is ignored (the man page: "The GNU C
//! Library ignores databases with unknown names"), as is a line without a colon; a later line
//! for the same database replaces an earlier one; a line whose source list does not parse (a
//! rule before any source, an unknown status or action, an unclosed `[`) configures the
//! database with no source at all, so its lookups are unavailable (observed).
//!
//! **Defaults** (the GNU C Library manual, "Notes on NSS Configuration File"): without a line,
//! `hosts` and `networks` use `files dns`, every other database `files`; `initgroups` uses the
//! `group` line (`nsswitch.conf(5)`).
//!
//! **Sources** ([`Service`]): `files` and `dns` are built in (`dns` is phase 10, part 4);
//! `compat` is `files` for `passwd`, `group` and `shadow` (its NIS inclusions are unavailable);
//! every other name (`db`, `nis`, `systemd`, `sss`, ...) is a module of the NSS module ABI
//! (F27, phase 14) and is unavailable until then.
//!
//! **Rules** (`[STATUS=ACTION]`, `[!STATUS=ACTION]`): the statuses are `success`, `notfound`,
//! `unavail` and `tryagain`; the actions `return`, `continue` and `merge`. Defaults: `success`
//! returns, the others continue. [`run`] applies them: `return` ends the lookup with this
//! source's result; `continue` discards it and asks the next source; `merge` (for `success`)
//! keeps it and asks the next source, whose success is merged into it; after a merge, later
//! failures are ignored and the merged result is returned (the manual).

use crate::error::Errno;

use super::is_space;

/// The databases (§16.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Database {
    /// `aliases` (§16.6).
    Aliases,
    /// `ethers` (§15.7).
    Ethers,
    /// `group` (§17.3).
    Group,
    /// `gshadow` (§17.4).
    Gshadow,
    /// `hosts` (§16.2, §16.3).
    Hosts,
    /// `initgroups` (`getgrouplist`, `initgroups`).
    Initgroups,
    /// `netgroup` (§16.5).
    Netgroup,
    /// `networks` (§16.3).
    Networks,
    /// `passwd` (§17.3).
    Passwd,
    /// `protocols` (§16.5).
    Protocols,
    /// `publickey` (Secure RPC; no lookup of this library reads it).
    Publickey,
    /// `rpc` (§16.5).
    Rpc,
    /// `services` (§16.5).
    Services,
    /// `shadow` (§17.4).
    Shadow,
}

/// The number of databases.
pub const DATABASES: usize = 14;

impl Database {
    /// Every database, in the order of [`Database::index`].
    pub const ALL: [Database; DATABASES] = [
        Database::Aliases,
        Database::Ethers,
        Database::Group,
        Database::Gshadow,
        Database::Hosts,
        Database::Initgroups,
        Database::Netgroup,
        Database::Networks,
        Database::Passwd,
        Database::Protocols,
        Database::Publickey,
        Database::Rpc,
        Database::Services,
        Database::Shadow,
    ];

    /// The name in `nsswitch.conf`.
    pub fn name(self) -> &'static [u8] {
        match self {
            Database::Aliases => b"aliases",
            Database::Ethers => b"ethers",
            Database::Group => b"group",
            Database::Gshadow => b"gshadow",
            Database::Hosts => b"hosts",
            Database::Initgroups => b"initgroups",
            Database::Netgroup => b"netgroup",
            Database::Networks => b"networks",
            Database::Passwd => b"passwd",
            Database::Protocols => b"protocols",
            Database::Publickey => b"publickey",
            Database::Rpc => b"rpc",
            Database::Services => b"services",
            Database::Shadow => b"shadow",
        }
    }

    /// The database named `name` (case-sensitive).
    pub fn from_name(name: &[u8]) -> Option<Database> {
        Database::ALL.iter().copied().find(|d| d.name() == name)
    }

    /// The position in [`Database::ALL`].
    pub fn index(self) -> usize {
        Database::ALL.iter().position(|&d| d == self).unwrap_or(0)
    }
}

/// A source (D-16.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Service {
    /// `files`: the database files under `/etc`.
    Files,
    /// `dns`: the DNS stub resolver (§16.4; phase 10, part 4).
    Dns,
    /// `compat`: `files` with the NIS inclusions of `passwd`, `group` and `shadow`, which are
    /// unavailable (no NIS client before phase 15).
    Compat,
    /// Any other name: a module of the NSS module ABI (F27, phase 14); unavailable until then.
    Module,
}

impl Service {
    /// The source a name in `nsswitch.conf` stands for.
    pub fn from_name(name: &[u8]) -> Service {
        match name {
            b"files" => Service::Files,
            b"dns" => Service::Dns,
            b"compat" => Service::Compat,
            _ => Service::Module,
        }
    }

    fn code(self) -> u8 {
        match self {
            Service::Files => 1,
            Service::Dns => 2,
            Service::Compat => 3,
            Service::Module => 4,
        }
    }

    fn from_code(c: u8) -> Option<Service> {
        match c {
            1 => Some(Service::Files),
            2 => Some(Service::Dns),
            3 => Some(Service::Compat),
            4 => Some(Service::Module),
            _ => None,
        }
    }
}

/// The outcome of asking one source.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// `success`: the entry.
    Success,
    /// `notfound`: the source works and has no such entry.
    NotFound,
    /// `unavail`: the source cannot be used (its file cannot be read, it is not built in).
    Unavail,
    /// `tryagain`: the source is temporarily unavailable.
    TryAgain,
}

impl Status {
    const ALL: [Status; 4] = [Status::Success, Status::NotFound, Status::Unavail, Status::TryAgain];

    fn index(self) -> usize {
        match self {
            Status::Success => 0,
            Status::NotFound => 1,
            Status::Unavail => 2,
            Status::TryAgain => 3,
        }
    }

    fn from_keyword(k: &[u8]) -> Option<Status> {
        let k = lower::<16>(k)?;
        Status::ALL.iter().copied().find(|s| s.keyword() == k.as_slice())
    }

    fn keyword(self) -> &'static [u8] {
        match self {
            Status::Success => b"success",
            Status::NotFound => b"notfound",
            Status::Unavail => b"unavail",
            Status::TryAgain => b"tryagain",
        }
    }
}

/// What a rule does with an outcome.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// `return`: the lookup ends with this outcome.
    Return,
    /// `continue`: ask the next source.
    Continue,
    /// `merge`: keep this result and merge the next source's into it.
    Merge,
}

impl Action {
    fn from_keyword(k: &[u8]) -> Option<Action> {
        match lower::<16>(k)?.as_slice() {
            b"return" => Some(Action::Return),
            b"continue" => Some(Action::Continue),
            b"merge" => Some(Action::Merge),
            _ => None,
        }
    }

    fn code(self) -> u8 {
        match self {
            Action::Return => 0,
            Action::Continue => 1,
            Action::Merge => 2,
        }
    }

    fn from_code(c: u8) -> Option<Action> {
        match c {
            0 => Some(Action::Return),
            1 => Some(Action::Continue),
            2 => Some(Action::Merge),
            _ => None,
        }
    }
}

/// A short keyword in lower case (`None` when longer than `N`).
fn lower<const N: usize>(k: &[u8]) -> Option<KeyBuf<N>> {
    let mut b = KeyBuf { buf: [0; N], len: k.len() };
    let dst = b.buf.get_mut(..k.len())?;
    for (d, s) in dst.iter_mut().zip(k) {
        *d = s.to_ascii_lowercase();
    }
    Some(b)
}

struct KeyBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> KeyBuf<N> {
    fn as_slice(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }
}

/// One source of a database and the actions for its four outcomes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step {
    /// The source.
    pub service: Service,
    /// The action per [`Status`] (`success`, `notfound`, `unavail`, `tryagain`).
    pub on: [Action; 4],
}

impl Step {
    /// `service` with the default actions.
    pub const fn new(service: Service) -> Step {
        Step { service, on: [Action::Return, Action::Continue, Action::Continue, Action::Continue] }
    }

    /// The action for `status`.
    pub fn action(&self, status: Status) -> Action {
        self.on.get(status.index()).copied().unwrap_or(Action::Continue)
    }
}

/// The most sources one database line may name (a line naming more does not parse).
pub const MAX_STEPS: usize = 16;

/// The bytes of an encoded [`Spec`] ([`Spec::encode`]).
pub const SPEC_BYTES: usize = 1 + MAX_STEPS * 5;

/// A database's sources, in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spec {
    steps: [Step; MAX_STEPS],
    len: usize,
}

impl Spec {
    /// No source: every lookup is unavailable.
    pub const EMPTY: Spec = Spec { steps: [Step::new(Service::Module); MAX_STEPS], len: 0 };

    /// The sources.
    pub fn steps(&self) -> &[Step] {
        self.steps.get(..self.len).unwrap_or(&[])
    }

    /// Whether `service` is among the sources.
    pub fn has(&self, service: Service) -> bool {
        self.steps().iter().any(|s| s.service == service)
    }

    /// Whether any source reads the database files (`files`, or `compat`).
    pub fn reads_files(&self) -> bool {
        self.has(Service::Files) || self.has(Service::Compat)
    }

    /// Whether any rule merges.
    pub fn merges(&self) -> bool {
        self.steps().iter().any(|s| s.on.contains(&Action::Merge))
    }

    fn push(&mut self, s: Step) -> Option<()> {
        *self.steps.get_mut(self.len)? = s;
        self.len = self.len.checked_add(1)?;
        Some(())
    }

    /// Sources from `names` with the default actions.
    pub fn of(names: &[Service]) -> Spec {
        let mut s = Spec::EMPTY;
        for &n in names {
            let _ = s.push(Step::new(n));
        }
        s
    }

    /// The source list of an `nsswitch.conf` line (after the colon), `None` when it does not
    /// parse.
    pub fn parse(text: &[u8]) -> Option<Spec> {
        let text = text.get(..text.iter().position(|&c| c == b'#').unwrap_or(text.len()))?;
        let mut spec = Spec::EMPTY;
        let mut i = 0usize;
        loop {
            while text.get(i).copied().is_some_and(is_space) {
                i = i.checked_add(1)?;
            }
            let Some(&c) = text.get(i) else { return Some(spec) };
            if c == b'[' {
                let close = text.get(i..)?.iter().position(|&c| c == b']')?;
                let body = text.get(i.checked_add(1)?..i.checked_add(close)?)?;
                let last = spec.len.checked_sub(1)?;
                let step = spec.steps.get_mut(last)?;
                rules(body, step)?;
                i = i.checked_add(close)?.checked_add(1)?;
            } else {
                let len = text.get(i..)?.iter().position(|&c| is_space(c) || c == b'[').unwrap_or(text.len().saturating_sub(i));
                let name = text.get(i..i.checked_add(len)?)?;
                spec.push(Step::new(Service::from_name(name)))?;
                i = i.checked_add(len)?;
            }
        }
    }

    /// The bytes of the spec, for process-wide storage in atomics.
    pub fn encode(&self) -> [u8; SPEC_BYTES] {
        let mut out = [0u8; SPEC_BYTES];
        if let Some(n) = out.first_mut() {
            *n = u8::try_from(self.len).unwrap_or(0);
        }
        for (chunk, s) in out.get_mut(1..).unwrap_or(&mut []).chunks_exact_mut(5).zip(self.steps()) {
            let [a, b, c, d] = s.on.map(Action::code);
            chunk.copy_from_slice(&[s.service.code(), a, b, c, d]);
        }
        out
    }

    /// The spec [`Spec::encode`] gave `bytes`.
    pub fn decode(bytes: &[u8]) -> Option<Spec> {
        let n = usize::from(*bytes.first()?);
        let mut spec = Spec::EMPTY;
        for chunk in bytes.get(1..)?.chunks_exact(5).take(n) {
            let on = [Action::from_code(*chunk.get(1)?)?, Action::from_code(*chunk.get(2)?)?, Action::from_code(*chunk.get(3)?)?, Action::from_code(*chunk.get(4)?)?];
            spec.push(Step { service: Service::from_code(*chunk.first()?)?, on })?;
        }
        (spec.len == n).then_some(spec)
    }
}

/// Apply the rules of one `[...]` group to `step`.
fn rules(body: &[u8], step: &mut Step) -> Option<()> {
    // White space next to an `=` is dropped; then each word is one `STATUS=ACTION` item.
    let mut norm = [0u8; 256];
    let mut len = 0usize;
    for (k, &c) in body.iter().enumerate() {
        if is_space(c) {
            let before = len.checked_sub(1).and_then(|p| norm.get(p)).copied();
            let after = body.get(k.saturating_add(1)..).unwrap_or(&[]).iter().copied().find(|&x| !is_space(x));
            if before == Some(b'=') || after == Some(b'=') {
                continue;
            }
        }
        *norm.get_mut(len)? = c;
        len = len.checked_add(1)?;
    }
    let mut any = false;
    for text in norm.get(..len)?.split(|&c| is_space(c)).filter(|w| !w.is_empty()) {
        let eq = text.iter().position(|&c| c == b'=')?;
        let (status, action) = (text.get(..eq)?, text.get(eq.checked_add(1)?..)?);
        let (negate, status) = match status.strip_prefix(b"!") {
            Some(s) => (true, s),
            None => (false, status),
        };
        let status = Status::from_keyword(status)?;
        let action = Action::from_keyword(action)?;
        for s in Status::ALL {
            if (s == status) != negate {
                *step.on.get_mut(s.index())? = action;
            }
        }
        any = true;
    }
    any.then_some(())
}

/// The source list a database uses when the configuration has no line for it.
pub fn default_spec(db: Database) -> Spec {
    match db {
        Database::Hosts | Database::Networks => Spec::of(&[Service::Files, Service::Dns]),
        _ => Spec::of(&[Service::Files]),
    }
}

/// A configuration: the line given for each database, if any.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    lines: [Option<Spec>; DATABASES],
}

impl Default for Config {
    fn default() -> Config {
        Config::EMPTY
    }
}

impl Config {
    /// No line for any database: every database uses its default.
    pub const EMPTY: Config = Config { lines: [None; DATABASES] };

    /// The configuration an `nsswitch.conf` text gives.
    pub fn parse(text: &[u8]) -> Config {
        let mut c = Config::EMPTY;
        for line in text.split(|&c| c == b'\n') {
            let line = line.get(..line.iter().position(|&c| c == b'#').unwrap_or(line.len())).unwrap_or(&[]);
            let Some(colon) = line.iter().position(|&c| c == b':') else { continue };
            let name = trim(line.get(..colon).unwrap_or(&[]));
            let Some(db) = Database::from_name(name) else { continue };
            let spec = Spec::parse(line.get(colon.saturating_add(1)..).unwrap_or(&[])).unwrap_or(Spec::EMPTY);
            c.set(db, Some(spec));
        }
        c
    }

    /// The line given for `db`.
    pub fn line(&self, db: Database) -> Option<Spec> {
        self.lines.get(db.index()).copied().flatten()
    }

    /// Give `db` the line `spec` (`None`: back to the default).
    pub fn set(&mut self, db: Database, spec: Option<Spec>) {
        if let Some(slot) = self.lines.get_mut(db.index()) {
            *slot = spec;
        }
    }

    /// The sources `db` uses.
    pub fn spec(&self, db: Database) -> Spec {
        self.resolve(db).0
    }

    /// The sources `db` uses, and whether they were inherited (`initgroups` without a line of
    /// its own uses `group`'s).
    pub fn resolve(&self, db: Database) -> (Spec, bool) {
        match (self.line(db), db) {
            (Some(s), _) => (s, false),
            (None, Database::Initgroups) => (self.spec(Database::Group), true),
            (None, _) => (default_spec(db), false),
        }
    }
}

fn trim(t: &[u8]) -> &[u8] {
    let start = t.iter().position(|&c| !is_space(c)).unwrap_or(t.len());
    let end = t.iter().rposition(|&c| !is_space(c)).map_or(start, |e| e.saturating_add(1));
    t.get(start..end).unwrap_or(&[])
}

/// Why a source or a whole lookup gave no entry (D-16.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LookupError {
    /// No such entry (`notfound`).
    NotFound,
    /// The name exists but has no entry of the kind asked for (a host without an address of
    /// the family): `notfound` for the rules, `NO_DATA`/`EAI_NODATA` for the caller.
    NoData,
    /// Temporarily unavailable (`tryagain`).
    TryAgain,
    /// The source cannot be used (`unavail`): not built in, not reachable.
    Unavailable,
    /// The source failed with a system error, typically its file could not be read (`unavail`
    /// for the rules; the error is reported to the caller).
    Os(Errno),
}

impl LookupError {
    /// The outcome the rules see.
    pub fn status(self) -> Status {
        match self {
            LookupError::NotFound | LookupError::NoData => Status::NotFound,
            LookupError::TryAgain => Status::TryAgain,
            LookupError::Unavailable | LookupError::Os(_) => Status::Unavail,
        }
    }
}

impl From<Errno> for LookupError {
    fn from(e: Errno) -> LookupError {
        LookupError::Os(e)
    }
}

/// A lookup's result: the entry, or why there is none.
pub type Answer<T> = core::result::Result<T, LookupError>;

/// Run one lookup through the sources of `spec` (D-16.1): `ask` asks one source; `merge` merges
/// a later success into a kept one (`[SUCCESS=merge]`; the `group` database's members). The
/// result of the source whose rule returns, the merged result, or the last failure; a failure
/// without a system error that follows one with an error reports the earlier error (so that a
/// database file that cannot be read stays visible behind an unavailable module).
pub fn run<T>(spec: &Spec, mut ask: impl FnMut(Service) -> Answer<T>, mut merge: impl FnMut(&mut T, T) -> crate::error::Result<()>) -> Answer<T> {
    let mut kept: Option<T> = None;
    let mut last = LookupError::Unavailable;
    let mut os: Option<Errno> = None;
    for step in spec.steps() {
        match ask(step.service) {
            Ok(v) => match step.action(Status::Success) {
                Action::Return => {
                    return match kept {
                        Some(mut k) => {
                            merge(&mut k, v)?;
                            Ok(k)
                        }
                        None => Ok(v),
                    };
                }
                Action::Continue => {}
                Action::Merge => {
                    kept = Some(match kept.take() {
                        Some(mut k) => {
                            merge(&mut k, v)?;
                            k
                        }
                        None => v,
                    });
                }
            },
            Err(e) => {
                if let LookupError::Os(x) = e {
                    os = Some(x);
                }
                last = e;
                if step.action(e.status()) == Action::Return {
                    break;
                }
            }
        }
    }
    match (kept, last, os) {
        (Some(k), _, _) => Ok(k),
        (None, LookupError::Unavailable, Some(x)) => Err(LookupError::Os(x)),
        (None, e, _) => Err(e),
    }
}

/// Gather a list through every source of `spec` until a rule returns (the `initgroups`
/// database: each success adds its entries). `inherited`: the spec is `group`'s, and its
/// `[NOTFOUND=return]` does not end the gathering (`nsswitch.conf(5)`). Failures add nothing.
pub fn gather(spec: &Spec, inherited: bool, mut ask: impl FnMut(Service) -> Answer<()>) {
    for step in spec.steps() {
        let status = match ask(step.service) {
            Ok(()) => Status::Success,
            Err(e) => e.status(),
        };
        let action = step.action(status);
        if action == Action::Return && !(inherited && status == Status::NotFound) {
            return;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn grammar() {
        let s = Spec::parse(b"dns [!UNAVAIL=return] files").unwrap();
        assert_eq!(s.steps().len(), 2);
        assert_eq!(s.steps()[0].on, [Action::Return, Action::Return, Action::Continue, Action::Return]);
        let s = Spec::parse(b" files [ NOTFOUND = return TryAgain=continue ] db # x").unwrap();
        assert_eq!(s.steps()[0].on, [Action::Return, Action::Return, Action::Continue, Action::Continue]);
        assert_eq!(s.steps()[1].service, Service::Module);
        assert_eq!(Spec::parse(b"[NOTFOUND=return] files"), None);
        assert_eq!(Spec::parse(b"files [FOO=return]"), None);
        assert_eq!(Spec::parse(b"files [NOTFOUND=return"), None);
        assert_eq!(Spec::parse(b"").unwrap().steps().len(), 0);
        assert_eq!(Spec::decode(&Spec::parse(b"files [SUCCESS=merge] dns").unwrap().encode()), Spec::parse(b"files [SUCCESS=merge] dns"));
        let c = Config::parse(b"HOSTS: dns\nhosts files\npasswd: files\npasswd: compat\nnosuch: x\ngroup: [x]\n");
        assert_eq!(c.spec(Database::Hosts), default_spec(Database::Hosts));
        assert_eq!(c.spec(Database::Passwd).steps()[0].service, Service::Compat);
        assert_eq!(c.spec(Database::Group).steps().len(), 0);
        assert_eq!(c.resolve(Database::Initgroups), (c.spec(Database::Group), true));
    }

    #[test]
    fn engine() {
        let spec = Spec::parse(b"a [SUCCESS=merge] b c").unwrap();
        let mut n = 0;
        let r = run(
            &spec,
            |_| {
                n += 1;
                if n == 3 { Err(LookupError::NotFound) } else { Ok(alloc::vec![n]) }
            },
            |k: &mut alloc::vec::Vec<i32>, v| {
                k.extend_from_slice(&v);
                Ok(())
            },
        );
        assert_eq!(r, Ok(alloc::vec![1, 2]));
        let spec = Spec::parse(b"files x").unwrap();
        let r: Answer<()> = run(&spec, |s| if s == Service::Files { Err(LookupError::Os(Errno::ENOENT)) } else { Err(LookupError::Unavailable) }, |_, _| Ok(()));
        assert_eq!(r, Err(LookupError::Os(Errno::ENOENT)));
        let spec = Spec::parse(b"x [UNAVAIL=return] files").unwrap();
        let r: Answer<()> = run(&spec, |s| if s == Service::Files { Ok(()) } else { Err(LookupError::Unavailable) }, |_, _| Ok(()));
        assert_eq!(r, Err(LookupError::Unavailable));
    }
}
