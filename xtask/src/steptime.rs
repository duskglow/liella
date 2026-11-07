//! Per-step wall times of the gate (XTASK-TEST-STEP-TIMES): `cargo xtask test`, and `cargo xtask affected --run` when
//! it escalates to FULL, print one line per step as it finishes and a summary, slowest first, at the end, so a step
//! that grows (the remez check went from ~100 s to ~1000 s in a day, GATE-TIME-PROFILE) shows in every gate log.
//!
//! The format is fixed so a log reader can grep it (the CI trend check reads these lines from the gate log):
//!
//! ```text
//! step: <name> <seconds>s            one per step, as it finishes (also when it failed)
//! step-summary: <name> <seconds>s    the same steps, slowest first, after the last step
//! step-total: <seconds>s             the sum of the steps
//! ```
//!
//! `<name>` has no whitespace (any is replaced by `-`); `<seconds>` has one decimal. Wall time is reported, never
//! asserted (CLAUDE.md rule 3): these lines are for trends, not limits. `crates/mathtools/src/bin/remez.rs` prints
//! the same `step:` line for each fit it checks (`remez/<fit>`), inside the gate's `remez` step.

use std::time::{Duration, Instant};

/// One `step:` line (without the newline).
pub fn line(name: &str, d: Duration) -> String {
    format!("step: {} {:.1}s", clean(name), d.as_secs_f64())
}

/// A step name without whitespace, so `<name>` is one field.
fn clean(name: &str) -> String {
    let s: String = name.split_whitespace().collect::<Vec<_>>().join("-");
    if s.is_empty() { "-".to_string() } else { s }
}

/// Parse a `step:` line back into its name and seconds (`None` for any other line).
pub fn parse(l: &str) -> Option<(&str, f64)> {
    let rest = l.trim_end().strip_prefix("step: ")?;
    let (name, secs) = rest.split_once(' ')?;
    let secs = secs.strip_suffix('s')?.parse::<f64>().ok()?;
    if name.is_empty() || !secs.is_finite() || secs < 0.0 { None } else { Some((name, secs)) }
}

/// The steps of one gate run, in the order they ran.
#[derive(Default)]
pub struct Steps {
    done: Vec<(String, Duration)>,
}

impl Steps {
    /// Run `f` as step `name`: its `step:` line is printed when it returns, whatever it returned.
    pub fn time<T>(&mut self, name: &str, f: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let r = f();
        self.add(name, started.elapsed());
        r
    }

    /// Record a step timed elsewhere (and print its line).
    pub fn add(&mut self, name: &str, d: Duration) {
        println!("{}", line(name, d));
        self.done.push((clean(name), d));
    }

    /// The summary lines: every step, slowest first (ties in run order), then the total.
    pub fn summary(&self) -> Vec<String> {
        let mut v: Vec<&(String, Duration)> = self.done.iter().collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.1));
        let mut out: Vec<String> = v.iter().map(|(n, d)| format!("step-summary: {n} {:.1}s", d.as_secs_f64())).collect();
        out.push(format!("step-total: {:.1}s", self.done.iter().map(|(_, d)| *d).sum::<Duration>().as_secs_f64()));
        out
    }

    pub fn print_summary(&self) {
        for l in self.summary() {
            println!("{l}");
        }
    }
}

#[cfg(test)]
mod tests {
    // no spec clause: the gate's own report format (XTASK-TEST-STEP-TIMES).
    use super::*;

    #[test]
    fn line_format_is_fixed_and_round_trips() {
        assert_eq!(line("remez", Duration::from_millis(1234)), "step: remez 1.2s");
        assert_eq!(line("nextest pass 0", Duration::from_secs(600)), "step: nextest-pass-0 600.0s");
        assert_eq!(line("  ", Duration::ZERO), "step: - 0.0s");
        assert_eq!(parse("step: remez/exp 12.5s"), Some(("remez/exp", 12.5)));
        assert_eq!(parse(&line("csys-gen check", Duration::from_millis(50))), Some(("csys-gen-check", 0.1)));
        for bad in ["step: remez", "step: remez 12.5", "step:  1.0s", "step-summary: remez 1.0s", "step: a b 1.0s", "x step: a 1.0s", "step: a -1.0s"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn summary_is_slowest_first_with_total() {
        let mut s = Steps::default();
        s.add("a", Duration::from_secs(1));
        s.add("b", Duration::from_secs(30));
        s.add("c", Duration::from_secs(1));
        s.add("d", Duration::from_secs(5));
        assert_eq!(s.summary(), ["step-summary: b 30.0s", "step-summary: d 5.0s", "step-summary: a 1.0s", "step-summary: c 1.0s", "step-total: 37.0s"]);
        assert_eq!(s.time("e", || 7), 7);
        assert_eq!(s.summary().len(), 6);
    }
}
