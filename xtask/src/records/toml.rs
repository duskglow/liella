//! The canonical TOML subset of the records (RECORDS-1, D-X.55).
//!
//! Every record file is valid TOML, but xtask reads only the one spelling it writes, so a record has exactly one form
//! and diffs stay uniform. One `key = value` per line, no blank lines, no comments, LF, final newline:
//!
//! * `key = "basic string"`: escapes `\\` and `\"` only; no control character other than tab;
//! * `key = 12`: a decimal integer without leading zeros;
//! * `key = ["a", "b"]`: strings only, `, ` between them; an empty list is written by leaving the key out;
//! * last, `body = '''` + newline + text + newline + `'''` (a multi-line literal string: the text is stored verbatim
//!   and may not contain `'''`); an empty body is `body = '''` + newline + `'''`. The value is the text without the
//!   final newline (a TOML reader sees the text plus one newline).
//!
//! `views.toml` uses the same lines in `[[view]]` tables separated by one blank line.
//!
//! [`parse`] is strict; [`parse_lenient`] (for `records fmt`) also takes keys in any order, blank and comment lines,
//! CRLF, `[]` and a missing final newline, and the caller rewrites the result with [`write`].

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Int(i64),
    List(Vec<String>),
    Body(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub value: Value,
    /// 1-based line of the key.
    pub line: usize,
}

/// A parse error: 1-based line and message.
pub type Error = (usize, String);

fn key_ok(k: &str) -> bool {
    !k.is_empty() && k.bytes().all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit()) && k.as_bytes()[0].is_ascii_lowercase()
}

/// One basic string at the start of `s`; returns the value and the rest after the closing quote.
fn basic(s: &str, n: usize) -> Result<(String, &str), Error> {
    let Some(mut rest) = s.strip_prefix('"') else { return Err((n, "expected a \"string\"".into())) };
    let mut out = String::new();
    loop {
        let mut chars = rest.chars();
        match chars.next() {
            None => return Err((n, "unterminated string".into())),
            Some('"') => return Ok((out, chars.as_str())),
            Some('\\') => match chars.next() {
                Some(c @ ('\\' | '"')) => out.push(c),
                _ => return Err((n, "only \\\\ and \\\" escapes are allowed".into())),
            },
            Some(c) if (c < ' ' && c != '\t') || c == '\u{7f}' => return Err((n, format!("control character U+{:04X} in a string", c as u32))),
            Some(c) => out.push(c),
        }
        rest = chars.as_str();
    }
}

fn value(v: &str, n: usize, lenient: bool) -> Result<Value, Error> {
    if v.starts_with('"') {
        let (s, rest) = basic(v, n)?;
        return if rest.is_empty() { Ok(Value::Str(s)) } else { Err((n, format!("text after the string: {rest:?}"))) };
    }
    if let Some(mut rest) = v.strip_prefix('[') {
        let mut items = Vec::new();
        if let Some(r) = rest.strip_prefix(']') {
            return if !r.is_empty() {
                Err((n, "text after the list".into()))
            } else if lenient {
                Ok(Value::List(items))
            } else {
                Err((n, "an empty list is written by leaving the key out".into()))
            };
        }
        loop {
            let (s, r) = basic(rest, n)?;
            items.push(s);
            if let Some(r) = r.strip_prefix(", ") {
                rest = r;
            } else if r == "]" {
                return Ok(Value::List(items));
            } else {
                return Err((n, "a list is [\"a\", \"b\"] (strings, \", \" between them)".into()));
            }
        }
    }
    if let Ok(i) = v.parse::<i64>()
        && i.to_string() == v
    {
        return Ok(Value::Int(i));
    }
    Err((n, format!("not a value of the record subset: {v:?}")))
}

/// Parse a record. `views` allows `[[view]]` table headers (returned as entries with key `[[view]]`).
fn parse_with(text: &str, lenient: bool, views: bool) -> Result<Vec<Entry>, Error> {
    let owned;
    let text = if lenient && text.contains('\r') {
        owned = text.replace("\r\n", "\n");
        owned.as_str()
    } else {
        text
    };
    if !lenient && !text.ends_with('\n') {
        return Err((text.lines().count().max(1), "no final newline".into()));
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let lines = if text.ends_with('\n') { &lines[..lines.len() - 1] } else { &lines[..] };
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let n = i + 1;
        let line = lines[i];
        i += 1;
        if line.contains('\r') {
            return Err((n, "CR in the line (LF line ends only)".into()));
        }
        if line.is_empty() || line.starts_with('#') {
            if lenient || (views && line.is_empty() && lines.get(i) == Some(&"[[view]]")) {
                continue;
            }
            return Err((n, if line.is_empty() { "blank line" } else { "comment line" }.into()));
        }
        if views && line == "[[view]]" {
            out.push(Entry { key: line.into(), value: Value::Int(0), line: n });
            continue;
        }
        let Some((k, v)) = line.split_once(" = ") else { return Err((n, "expected `key = value`".into())) };
        if !key_ok(k) {
            return Err((n, format!("bad key {k:?}")));
        }
        if v == "'''" {
            let mut body: Vec<&str> = Vec::new();
            loop {
                let Some(&l) = lines.get(i) else { return Err((n, "body not closed by a line `'''`".into())) };
                i += 1;
                if l == "'''" {
                    break;
                }
                if l.contains("'''") {
                    return Err((i, "the body may not contain '''".into()));
                }
                body.push(l);
            }
            out.push(Entry { key: k.into(), value: Value::Body(body.join("\n")), line: n });
            if i < lines.len() && !views && !lenient {
                return Err((i + 1, "the body must be the last key".into()));
            }
            continue;
        }
        if v.starts_with("'''") || v.starts_with('\'') {
            return Err((n, "a literal string is only `body = '''` on its own line".into()));
        }
        out.push(Entry { key: k.into(), value: value(v, n, lenient)?, line: n });
    }
    Ok(out)
}

pub fn parse(text: &str) -> Result<Vec<Entry>, Error> {
    parse_with(text, false, false)
}

pub fn parse_lenient(text: &str) -> Result<Vec<Entry>, Error> {
    parse_with(text, true, false)
}

/// `views.toml`: the `[[view]]` tables, each a list of entries.
pub fn parse_tables(text: &str) -> Result<Vec<Vec<Entry>>, Error> {
    let mut tables: Vec<Vec<Entry>> = Vec::new();
    for e in parse_with(text, false, true)? {
        if e.key == "[[view]]" {
            tables.push(Vec::new());
        } else if let Some(t) = tables.last_mut() {
            t.push(e);
        } else {
            return Err((e.line, "a key before the first [[view]]".into()));
        }
    }
    Ok(tables)
}

pub fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The canonical text of one `key = value` line (a body ends with its closing `'''` line).
pub fn line(key: &str, v: &Value) -> String {
    match v {
        Value::Str(s) => format!("{key} = {}\n", quote(s)),
        Value::Int(i) => format!("{key} = {i}\n"),
        Value::List(l) => format!("{key} = [{}]\n", l.iter().map(|s| quote(s)).collect::<Vec<_>>().join(", ")),
        Value::Body(b) if b.is_empty() => format!("{key} = '''\n'''\n"),
        Value::Body(b) => format!("{key} = '''\n{b}\n'''\n"),
    }
}

/// The canonical text of a record (entries already in schema order; empty lists left out).
pub fn write(entries: &[(String, Value)]) -> String {
    entries.iter().filter(|(_, v)| !matches!(v, Value::List(l) if l.is_empty())).map(|(k, v)| line(k, v)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REC: &str = "id = \"D-1.6a\"\nspec = \"a \\\\| b \\\"q\\\"\"\nn = 12\ntests = [\"a.rs::t\", \"b.rs\"]\nbody = '''\nverbatim \\| `x` 'q'\n'''\n";

    #[test]
    fn canonical_round_trip() {
        let e = parse(REC).expect("parses");
        assert_eq!(e[1].value, Value::Str("a \\| b \"q\"".into()));
        assert_eq!(e[2].value, Value::Int(12));
        assert_eq!(e[3].value, Value::List(vec!["a.rs::t".into(), "b.rs".into()]));
        assert_eq!(e[4].value, Value::Body("verbatim \\| `x` 'q'".into()));
        let pairs: Vec<(String, Value)> = e.into_iter().map(|e| (e.key, e.value)).collect();
        assert_eq!(write(&pairs), REC);
        assert_eq!(parse("body = '''\n'''\n").expect("empty body")[0].value, Value::Body(String::new()));
    }

    #[test]
    fn other_spellings_are_errors_with_their_line() {
        for (bad, line) in [
            ("id = 'x'\n", 1),
            ("id = \"x\"\n\nb = \"y\"\n", 2),
            ("id = \"x\" # c\n", 1),
            ("id=\"x\"\n", 1),
            ("a = \"x\"\nt = []\n", 2),
            ("a = \"\\n\"\n", 1),
            ("a = 012\n", 1),
            ("a = [\"x\",\"y\"]\n", 1),
            ("a = \"x\"", 1),
            ("body = '''\nx\n'''\nid = \"y\"\n", 4),
            ("body = '''\nx\n", 1),
            ("a = \"x\"\r\n", 1),
        ] {
            assert_eq!(parse(bad).map_err(|e| e.0), Err(line), "{bad:?}");
        }
    }

    #[test]
    fn lenient_reads_what_fmt_rewrites() {
        let e = parse_lenient("# c\r\nb = []\r\n\r\na = \"x\"").expect("lenient");
        assert_eq!(e.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    }

    #[test]
    fn view_tables() {
        let t = parse_tables("[[view]]\nname = \"a\"\n\n[[view]]\nname = \"b\"\n").expect("tables");
        assert_eq!(t.len(), 2);
        assert!(parse_tables("[[view]]\nname = \"a\"\n\n\n[[view]]\n").is_err());
    }
}
