//! The steps both examples run, one per stateful service, each printing one line: `ok <step>:
//! <what it did>` or `FAIL <step>: <why>`. `cargo xtask native` runs the static example as
//!
//! ```text
//! env -i LIELLA_NATIVE_GREETING=hello TZ=CET-1CEST,M3.5.0,M10.5.0/3 native_static <dir> alpha beta
//! ```
//!
//! and compares its output with the lines it expects (xtask/src/native_examples.rs). `<dir>` is an
//! existing, empty directory for the file round trip.

#![forbid(unsafe_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use liella_native::fd::{Stdio, Whence};
use liella_native::secure::{secure_zero, SecureVec};
use liella_native::{aio, clock, env, fs, getpass, locale, nscache, process, tz, Errno};

/// A step: what it did, or why it failed.
pub(crate) type Step = fn() -> Result<String, String>;

/// An error of a native call, with what was being done.
fn at(what: &'static str) -> impl Fn(Errno) -> String {
    move |e| format!("{what}: {e}")
}

/// `ok` unless `cond`, then `why`.
fn check(cond: bool, why: &str) -> Result<(), String> {
    if cond { Ok(()) } else { Err(String::from(why)) }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The heap as the global allocator: growth by `realloc` at the minimum alignment, and a block
/// aligned to 64 that grows (allocate, copy, free).
fn heap() -> Result<String, String> {
    let mut words: Vec<u64> = Vec::new();
    for i in 0..1000u64 {
        words.push(i);
    }
    let sum: u64 = words.iter().sum();
    check(sum == 499_500, "the words do not add up")?;

    #[derive(Clone, Copy)]
    #[repr(align(64))]
    struct Line(u64);
    let mut lines: Vec<Line> = Vec::new();
    for i in 0..100u64 {
        lines.push(Line(i));
        check(lines.as_ptr().addr() & 63 == 0, "a 64-aligned block is misaligned")?;
    }
    check(lines.iter().enumerate().all(|(i, l)| u64::try_from(i).ok() == Some(l.0)), "a 64-aligned block lost its contents")?;
    Ok(format!("1000 words (sum {sum}); 100 lines aligned to 64 after growth"))
}

/// Secrets: a buffer in secure heap blocks, grown past a page (the heap moves it and zeroes
/// the block it leaves), truncated (the bytes removed are zeroed at once) and dropped (the
/// heap zeroes its block); a buffer that starts zeroed; and a stack buffer zeroed by
/// `secure_zero`, which the optimiser cannot remove.
fn secrets() -> Result<String, String> {
    const SECRET: &[u8] = b"correct horse battery staple";
    let mut key = SecureVec::from_slice(SECRET).map_err(at("from_slice"))?;
    let (first, at_first) = (key.capacity(), key.as_ptr().addr());
    for _ in 0..5 {
        key.extend_from_slice(&[0x5a; 1000]).map_err(at("extend_from_slice"))?;
    }
    let grown = key.len();
    check(key.starts_with(SECRET) && key.iter().skip(SECRET.len()).all(|&b| b == 0x5a), "the secret changed as it grew")?;
    check(key.as_ptr().addr() != at_first, "a buffer grown past a page did not move")?;
    let cap = key.capacity();
    key.truncate(SECRET.len());
    check(&key[..] == SECRET && key.capacity() == cap, "truncate lost the secret or the block")?;
    drop(key);
    let fresh = SecureVec::zeroed(32).map_err(at("zeroed"))?;
    check(fresh.len() == 32 && fresh.iter().all(|&b| b == 0), "a zeroed buffer is not zero")?;
    let mut stack = [0xa5u8; 32];
    secure_zero(&mut stack);
    check(stack == [0; 32], "secure_zero left a byte")?;
    Ok(format!("{}-byte secret grown to {grown} bytes (capacity {first} to {cap}, moved by the heap), truncated intact; zeroed buffer of {}; stack buffer of {} zeroed", SECRET.len(), fresh.len(), stack.len()))
}

/// The arguments: the program name, the directory, then the rest.
fn args() -> Result<String, String> {
    let count = process::arg_count().map_err(at("arg_count"))?;
    let all: Vec<Vec<u8>> = process::args().collect::<Result<_, _>>().map_err(at("args"))?;
    check(all.len() == count, "args() and arg_count() disagree")?;
    check(process::arg(count).map_err(at("arg"))?.is_none(), "an argument past the count")?;
    let rest: Vec<String> = all.iter().skip(2).map(|a| text(a.as_slice())).collect();
    Ok(format!("{count} ({})", rest.join(" ")))
}

/// The environment: read, set (with and without overwrite), unset, count, iterate.
fn environment() -> Result<String, String> {
    let greeting = env::get(b"LIELLA_NATIVE_GREETING").map_err(at("get"))?.ok_or("LIELLA_NATIVE_GREETING is not set")?;
    let mut small = [0u8; 2];
    let fit = env::get_into(b"LIELLA_NATIVE_GREETING", &mut small).map_err(at("get_into"))?;
    check(fit == Some(liella_native::Fit::Needed(greeting.len())), "get_into did not report the length needed")?;

    env::set(b"LIELLA_NATIVE_SET", b"first", true).map_err(at("set"))?;
    env::set(b"LIELLA_NATIVE_SET", b"second", false).map_err(at("set"))?;
    check(env::get(b"LIELLA_NATIVE_SET").map_err(at("get"))?.as_deref() == Some(b"first"), "set without overwrite replaced the value")?;
    let long = [b'v'; 1000];
    env::set(b"LIELLA_NATIVE_SET", &long, true).map_err(at("set"))?;
    check(env::get(b"LIELLA_NATIVE_SET").map_err(at("get"))?.as_deref() == Some(&long[..]), "a 1000-byte value did not come back")?;
    // The environment contract (D-1.12): off by default; while on, writers keep the table current.
    check(!env::contract(true), "the environment contract was on before the program turned it on")?;
    env::set(b"LIELLA_NATIVE_SET", b"contract", true).map_err(at("set"))?;
    env::reload();
    check(env::get(b"LIELLA_NATIVE_SET").map_err(at("get"))?.as_deref() == Some(b"contract"), "a setenv under the contract was not seen")?;
    check(env::contract(false), "turning the contract off did not report it on")?;
    check(env::set(b"A=B", b"x", true) == Err(Errno::EINVAL), "a name with '=' was accepted")?;
    env::unset(b"LIELLA_NATIVE_SET").map_err(at("unset"))?;
    check(env::get(b"LIELLA_NATIVE_SET").map_err(at("get"))?.is_none(), "unset left the variable")?;

    let count = env::count().map_err(at("count"))?;
    let entries: Vec<Vec<u8>> = env::vars().collect::<Result<_, _>>().map_err(at("vars"))?;
    check(entries.len() == count, "vars() and count() disagree")?;
    check(entries.iter().any(|e| e.as_slice() == b"LIELLA_NATIVE_GREETING=hello"), "the greeting is not among the entries")?;
    Ok(format!("LIELLA_NATIVE_GREETING={}; set, overwrite, unset; {count} entries", text(&greeting)))
}

/// A file round trip in the directory given as the first argument.
fn file() -> Result<String, String> {
    let dir = process::arg(1).map_err(at("arg"))?.ok_or("no directory argument")?;
    let path = |name: &[u8]| -> Vec<u8> { [dir.as_slice(), &b"/"[..], name].concat() };
    let sub = path(b"sub");
    let first = path(b"sub/first");
    let second = path(b"sub/second");
    let data = b"liella native round trip\n";

    fs::mkdir(&sub, 0o755).map_err(at("mkdir"))?;
    check(fs::stat(&sub).map_err(at("stat"))?.is_dir(), "the directory is not one")?;
    let f = fs::open(&first, fs::O_RDWR | fs::O_CREAT | fs::O_EXCL | fs::O_CLOEXEC, 0o644).map_err(at("open"))?;
    f.write_all(data).map_err(at("write_all"))?;
    let n = f.pwrite(b"LIELLA", 0).map_err(at("pwrite"))?;
    check(n == 6, "pwrite was short")?;
    let end = f.seek(0, Whence::Current).map_err(at("seek"))?;
    check(usize::try_from(end).ok() == Some(data.len()), "pwrite moved the offset")?;
    f.seek(0, Whence::Set).map_err(at("seek"))?;
    let mut back = [0u8; 64];
    let n = f.read(&mut back).map_err(at("read"))?;
    check(back.get(..n) == Some(&b"LIELLA native round trip\n"[..]), "read gave other bytes")?;
    let mut word = [0u8; 6];
    f.pread(&mut word, 7).map_err(at("pread"))?;
    check(&word == b"native", "pread gave other bytes")?;
    let st = f.stat().map_err(at("fstat"))?;
    check(st.is_file() && usize::try_from(st.size).ok() == Some(data.len()) && st.permissions() & 0o600 == 0o600, "fstat disagrees")?;
    f.close().map_err(at("close"))?;

    check(fs::open(&first, fs::O_RDWR | fs::O_CREAT | fs::O_EXCL, 0o644).err() == Some(Errno::EEXIST), "O_EXCL opened an existing file")?;
    fs::rename(&first, &second).map_err(at("rename"))?;
    check(fs::lstat(&first).err() == Some(Errno::ENOENT), "the old name survived the rename")?;
    check(fs::stat(&second).map_err(at("stat"))?.size == st.size, "the renamed file changed")?;
    check(fs::rmdir(&sub).err() == Some(Errno::ENOTEMPTY), "rmdir removed a full directory")?;
    fs::unlink(&second).map_err(at("unlink"))?;
    fs::rmdir(&sub).map_err(at("rmdir"))?;
    check(fs::stat(&sub).err() == Some(Errno::ENOENT), "the directory survived rmdir")?;
    check(fs::open(b"a\0b", fs::O_RDONLY, 0).err() == Some(Errno::EINVAL), "a path with a NUL was accepted")?;
    Ok(format!("{} bytes written, read back, renamed, removed", data.len()))
}

/// The clocks.
fn clocks() -> Result<String, String> {
    let now = clock::realtime().map_err(at("realtime"))?;
    // 2026-01-01 00:00:00 UTC.
    check(now.sec >= 1_767_225_600 && (0..1_000_000_000).contains(&now.nsec), "the current time is implausible")?;
    let a = clock::monotonic().map_err(at("monotonic"))?;
    let b = clock::monotonic().map_err(at("monotonic"))?;
    check(b >= a, "the monotonic clock went back")?;
    Ok(String::from("realtime after 2026-01-01, monotonic non-decreasing"))
}

fn local(tm: &tz::Tm) -> String {
    let abbr = tm.abbr().map(text).unwrap_or_default();
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} {abbr}", tm.year, tm.month, tm.day, tm.hour, tm.minute, tm.second)
}

/// The time zone `TZ` names, in both directions; then `TZ` changed through the environment,
/// which the zone follows (one environment, one zone).
fn zone() -> Result<String, String> {
    let t = 1_782_907_200; // 2026-07-01 12:00:00 UTC
    let summer = tz::to_local(t).map_err(at("to_local"))?;
    let (back, _) = tz::from_local(&summer).map_err(at("from_local"))?;
    check(back == t, "from_local(to_local(t)) is not t")?;
    let (winter_t, winter) = tz::from_local(&tz::Tm::civil(2026, 1, 15, 13, 0, 0)).map_err(at("from_local"))?;
    let tz_before = env::get(b"TZ").map_err(at("get"))?;
    env::set(b"TZ", b"JST-9", true).map_err(at("set"))?;
    let japan = tz::to_local(t).map_err(at("to_local"));
    match tz_before {
        Some(v) => env::set(b"TZ", &v, true).map_err(at("set"))?,
        None => env::unset(b"TZ").map_err(at("unset"))?,
    }
    let japan = japan?;
    // Reloading the zone data (D-X.13) changes nothing when no file changed.
    tz::invalidate();
    check(tz::to_local(t).map_err(at("to_local"))? == summer, "the zone changed after invalidate")?;
    Ok(format!(
        "{t} is {} (+{}, dst {}, weekday {}, yearday {}); {} is {winter_t}; with TZ=JST-9 {}",
        local(&summer), summer.utc_offset, summer.isdst, summer.weekday, summer.yearday, local(&winter), local(&japan)
    ))
}

/// The stateless clean API, compiled into the program.
fn clean() -> Result<String, String> {
    let c = liella_native::liella::time::utc(1_782_907_200).map_err(at("utc"))?;
    Ok(format!("1782907200 is {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", c.year, c.month, c.day, c.hour, c.minute, c.second))
}

/// The current locale.
fn current_locale() -> Result<String, String> {
    let name = locale::name(locale::Category::All).map_err(at("name"))?;
    let ctype = locale::name(locale::Category::Ctype).map_err(at("name"))?;
    let point = locale::decimal_point().map_err(at("decimal_point"))?;
    let mut tiny = [0u8; 0];
    let fit = locale::decimal_point_into(&mut tiny).map_err(at("decimal_point_into"))?;
    check(fit == liella_native::Fit::Needed(point.len()), "decimal_point_into did not report the length needed")?;
    // Reloading the locale data (D-X.13) keeps the global locale.
    locale::invalidate().map_err(at("invalidate"))?;
    check(locale::name(locale::Category::All).map_err(at("name"))? == name, "the locale changed after invalidate")?;
    Ok(format!("LC_ALL={}, LC_CTYPE={}, decimal point \"{}\"", text(&name), text(&ctype), text(&point)))
}

/// The process ID.
fn pid() -> Result<String, String> {
    let id = process::id().map_err(at("id"))?;
    check(id > 0, "the process ID is 0")?;
    Ok(String::from("pid is positive"))
}

/// The name-service cache's switches (D-16.10; the example makes no lookup, so nothing is
/// mapped or created).
fn names_cache() -> Result<String, String> {
    let s = nscache::status().map_err(at("status"))?;
    check(s.enabled == 1 && s.mode() == nscache::Mode::Idle && s.has(nscache::Category::Hosts), "the cache is not enabled and idle")?;
    check(nscache::set_category(nscache::Category::Hosts, false) == Ok(true), "set_category off")?;
    check(!nscache::status().map_err(at("status"))?.has(nscache::Category::Hosts), "hosts still enabled")?;
    check(nscache::set_category(nscache::Category::Hosts, true) == Ok(false), "set_category on")?;
    nscache::invalidate(None).map_err(at("invalidate"))?;
    check(nscache::enable(false), "enable(false) did not report it enabled")?;
    check(nscache::status().map_err(at("status"))?.mode() == nscache::Mode::Off, "not off")?;
    check(!nscache::enable(true), "enable(true) did not report it disabled")?;
    Ok(format!("enabled and idle, {} categories; switched off and on, invalidated", s.categories.count_ones()))
}

/// POSIX AIO's engine: the worker threads, chosen before any request (`liella_aio_use_threads`;
/// asking again is no error).
fn aio_engine() -> Result<String, String> {
    aio::use_threads().map_err(at("use_threads"))?;
    aio::use_threads().map_err(at("use_threads again"))?;
    Ok(String::from("worker threads chosen before the first request"))
}

/// `getpass`'s bound (`liella_getpass_set_limit`): the default, a new bound, the default again.
fn pass_bound() -> Result<String, String> {
    check(getpass::set_limit(4096) == Ok(getpass::DEFAULT_LIMIT), "the default bound was not in force")?;
    check(getpass::set_limit(0) == Ok(4096), "the new bound was not in force")?;
    check(getpass::set_limit(0) == Ok(getpass::DEFAULT_LIMIT), "0 did not restore the default")?;
    check(getpass::set_limit(usize::MAX) == Err(Errno::EINVAL), "a bound above isize::MAX was accepted")?;
    Ok(format!("default {} bytes; set to 4096 and back", getpass::DEFAULT_LIMIT))
}

/// The steps of both examples.
pub(crate) const STEPS: [(&str, Step); 13] = [
    ("heap", heap),
    ("secure", secrets),
    ("args", args),
    ("env", environment),
    ("file", file),
    ("clock", clocks),
    ("tz", zone),
    ("clean api", clean),
    ("locale", current_locale),
    ("process", pid),
    ("nscache", names_cache),
    ("aio", aio_engine),
    ("getpass", pass_bound),
];

/// Run `steps`, print a line for each and a summary, and exit through
/// `liella_native::process::exit`: status 0 if every step passed, else 1.
pub(crate) fn run(steps: &[(&str, Step)]) -> ! {
    let mut out = Stdio::OUT;
    let _ = writeln!(out, "native example: linkage {}", liella_native::LINKAGE.name());
    let mut failed = 0usize;
    for (name, step) in steps {
        match step() {
            Ok(what) => {
                let _ = writeln!(out, "ok {name}: {what}");
            }
            Err(why) => {
                failed = failed.saturating_add(1);
                let _ = writeln!(out, "FAIL {name}: {why}");
            }
        }
    }
    let _ = writeln!(out, "native example: {} of {} steps passed", steps.len().saturating_sub(failed), steps.len());
    process::exit(if failed == 0 { 0 } else { 1 })
}

/// A panic is a bug in the example: report it and exit with status 101. Unused by a program of
/// the Rust dylib, which defines the panic handler.
#[allow(dead_code)]
pub(crate) fn panicked(info: &core::panic::PanicInfo<'_>) -> ! {
    let mut err = Stdio::ERR;
    let _ = writeln!(err, "native example panicked: {info}");
    process::exit(101)
}
