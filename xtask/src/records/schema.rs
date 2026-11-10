//! Record kinds, their keys (in canonical order), id shapes and the id order of the views.

use super::toml::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Decision,
    Compat,
    Saturation,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Decision, Kind::Compat, Kind::Saturation];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Decision => "decision",
            Kind::Compat => "compat",
            Kind::Saturation => "saturation",
        }
    }

    pub fn from_name(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == s)
    }

    /// The directory under `records/`.
    pub fn dir(self) -> &'static str {
        match self {
            Kind::Decision => "decisions",
            Kind::Compat => "compat",
            Kind::Saturation => "saturation",
        }
    }

    /// Every key of the kind, in canonical order. `series` is for placeholder records under `records/pending/` only.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Kind::Decision => &["id", "series", "section", "spec", "ann", "pattern", "status", "supersedes", "amends", "story", "date", "tests", "body"],
            Kind::Compat => &["id", "series", "behaviour", "glibc", "liella", "restore", "cost", "status", "status_text", "decisions", "story", "date", "body"],
            Kind::Saturation => &["id", "series", "section", "row", "now", "limit", "reason_class", "basis", "source", "tried", "flag", "open", "status", "supersedes", "story", "date", "body"],
        }
    }

    /// Keys every record of the kind has (the view's columns come on top).
    pub fn required(self) -> &'static [&'static str] {
        match self {
            Kind::Compat => &["id", "status", "body"],
            _ => &["id", "section", "status", "body"],
        }
    }

    pub fn statuses(self) -> &'static [&'static str] {
        match self {
            Kind::Compat => &["registered", "implemented", "partial", "withdrawn"],
            _ => &["active", "withdrawn"],
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            Kind::Decision => "D-NEW",
            Kind::Compat => "COMPAT-NEW",
            Kind::Saturation => "SR-NEW",
        }
    }

    pub fn of_id(id: &str) -> Option<Kind> {
        if id.starts_with("COMPAT-") {
            Some(Kind::Compat)
        } else if id.starts_with("SR-") {
            Some(Kind::Saturation)
        } else if id.starts_with("D-") {
            Some(Kind::Decision)
        } else {
            None
        }
    }
}

/// The type a key holds.
pub fn value_type_ok(key: &str, v: &Value) -> bool {
    match key {
        "supersedes" | "amends" | "tests" | "decisions" => matches!(v, Value::List(_)),
        "body" => matches!(v, Value::Body(_)),
        _ => matches!(v, Value::Str(_)),
    }
}

/// The parts of an id: series (`D-10`, `D-X`, `D-H`, `COMPAT`, `SR`), number and optional letter suffix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdParts {
    pub series: String,
    pub num: u64,
    pub suffix: Option<u8>,
}

fn number(s: &str) -> Option<u64> {
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Parse an id of `kind`: `D-<n>.<m>[a-z]`, `D-X.<m>`, `D-C0.<m>`, `D-T.<m>`, `D-H<m>`; `COMPAT-<n>`; `SR-<n>`.
pub fn parse_id(kind: Kind, id: &str) -> Option<IdParts> {
    let simple = |prefix: &str, series: &str| id.strip_prefix(prefix).and_then(number).map(|num| IdParts { series: series.into(), num, suffix: None });
    match kind {
        Kind::Compat => simple("COMPAT-", "COMPAT"),
        Kind::Saturation => simple("SR-", "SR"),
        Kind::Decision => {
            let rest = id.strip_prefix("D-")?;
            if let Some(h) = rest.strip_prefix('H') {
                return number(h).map(|num| IdParts { series: "D-H".into(), num, suffix: None });
            }
            let (s, m) = rest.split_once('.')?;
            if !(number(s).is_some() || ["X", "C0", "T"].contains(&s)) {
                return None;
            }
            let (digits, suffix) = match m.as_bytes().last() {
                Some(c) if c.is_ascii_lowercase() => (&m[..m.len() - 1], Some(*c)),
                _ => (m, None),
            };
            number(digits).map(|num| IdParts { series: format!("D-{s}"), num, suffix })
        }
    }
}

/// The render order of an id inside a view: the series' position in the view's `series` list, then the number, then
/// the suffix (D-1.6 < D-1.6a < D-1.7).
pub fn sort_key(p: &IdParts, series: &[String]) -> (usize, u64, u8) {
    (series.iter().position(|s| *s == p.series).unwrap_or(series.len()), p.num, p.suffix.unwrap_or(0))
}

/// The decision table (view section) an id belongs to by default: `D-10` -> `s10`, `D-27` -> `s26-29`, `D-X` -> `x`.
pub fn default_section(series: &str) -> String {
    match series {
        "D-H" | "D-T" => "h".into(),
        "D-X" => "x".into(),
        "D-C0" => "c0".into(),
        s => match s.strip_prefix("D-").and_then(|n| n.parse::<u32>().ok()) {
            Some(n) if (26..=29).contains(&n) => "s26-29".into(),
            Some(n) => format!("s{n:02}"),
            None => String::new(),
        },
    }
}

pub fn date_ok(d: &str) -> bool {
    let b = d.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !d.bytes().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) {
        return false;
    }
    let (m, day) = (d[5..7].parse::<u32>().unwrap_or(0), d[8..10].parse::<u32>().unwrap_or(0));
    (1..=12).contains(&m) && (1..=31).contains(&day)
}

pub fn story_ok(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'-')
}

/// A cell's text: one line, no `'''`, and every `|` escaped as `\|` (the renderer stores cells verbatim, and GitHub
/// splits a cell at a bare `|`, even inside a code span).
pub fn cell_problem(s: &str) -> Option<&'static str> {
    if s.contains('\n') {
        return Some("a newline");
    }
    if s.contains("'''") {
        return Some("'''");
    }
    let b = s.as_bytes();
    if b.iter().enumerate().any(|(i, c)| *c == b'|' && (i == 0 || b[i - 1] != b'\\')) {
        return Some("a bare `|` (write `\\|`)");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_shapes() {
        let p = |k, s| parse_id(k, s).map(|p| (p.series, p.num, p.suffix));
        assert_eq!(p(Kind::Decision, "D-1.6a"), Some(("D-1".into(), 6, Some(b'a'))));
        assert_eq!(p(Kind::Decision, "D-H12"), Some(("D-H".into(), 12, None)));
        assert_eq!(p(Kind::Decision, "D-C0.7"), Some(("D-C0".into(), 7, None)));
        assert_eq!(p(Kind::Compat, "COMPAT-20"), Some(("COMPAT".into(), 20, None)));
        for bad in ["D-1", "D-01.2", "D-1.02", "D-Y.1", "D-1.6ab", "D-H", "D-NEW", "D-1.0"] {
            assert_eq!(p(Kind::Decision, bad), None, "{bad}");
        }
        assert_eq!(p(Kind::Saturation, "SR-0"), None);
    }

    #[test]
    fn order_is_series_number_suffix() {
        let series = vec!["D-3".to_string(), "D-4".to_string()];
        let mut ids = vec!["D-4.2", "D-1.6", "D-4.10", "D-3.9", "D-4.2a", "D-4.1"];
        ids.sort_by_key(|i| sort_key(&parse_id(Kind::Decision, i).expect("id"), &series));
        assert_eq!(ids, ["D-3.9", "D-4.1", "D-4.2", "D-4.2a", "D-4.10", "D-1.6"]);
    }

    #[test]
    fn cells_dates_sections() {
        assert_eq!(cell_problem("a \\| b"), None);
        assert!(cell_problem("`a|b`").is_some() && cell_problem("|x").is_some() && cell_problem("a\nb").is_some());
        assert!(date_ok("2026-11-08") && !date_ok("2026-13-01") && !date_ok("2026-1-08"));
        assert_eq!(default_section("D-27"), "s26-29");
        assert_eq!(default_section("D-4"), "s04");
    }
}
