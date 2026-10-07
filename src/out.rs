//! Output shared by every verb: stdout that ends quietly on a closed pipe,
//! the bracketed `[hail …]` head of envelopes and brief lines, and aligned
//! tables.

use std::fmt::Display;
use std::io::Write as _;

/// Print to stdout; a reader that went away (`hail inbox | head -1`) ends the
/// output quietly instead of panicking.
macro_rules! out {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = write!(std::io::stdout(), $($t)*);
    }};
}

macro_rules! outln {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

/// `[hail <kind> key:value …] text`: the head of an envelope (fields named,
/// `kind:<k>`) or of a brief line (kind bare). Absent optional fields are
/// left out.
pub struct Tag(String);

impl Tag {
    /// A brief line: `[hail fyi …`.
    pub fn brief(kind: impl Display) -> Self {
        Self(format!("[hail {kind}"))
    }

    /// An envelope: `[hail kind:fyi …`.
    pub fn envelope(kind: impl Display) -> Self {
        Self(format!("[hail kind:{kind}"))
    }

    #[must_use]
    pub fn field(mut self, key: &str, value: impl Display) -> Self {
        use std::fmt::Write as _;
        let _ = write!(self.0, " {key}:{value}");
        self
    }

    #[must_use]
    pub fn opt(self, key: &str, value: Option<&str>) -> Self {
        match value.filter(|v| !v.is_empty()) {
            Some(v) => self.field(key, v),
            None => self,
        }
    }

    /// Close the bracket and append the text.
    pub fn text(self, text: &str) -> String {
        format!("{}] {text}", self.0)
    }
}

/// Columns padded to their widest cell; the last column is not padded.
pub struct Table {
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(header: &[&str]) -> Self {
        Self {
            rows: vec![header.iter().map(ToString::to_string).collect()],
        }
    }

    pub fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    pub fn print(&self) {
        let cols = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..cols)
            .map(|c| {
                self.rows
                    .iter()
                    .filter_map(|r| r.get(c))
                    .map(|s| s.chars().count())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let mut stdout = std::io::stdout().lock();
        for row in &self.rows {
            let last = row.len().saturating_sub(1);
            let line: String = row
                .iter()
                .enumerate()
                .map(|(c, cell)| {
                    if c == last {
                        cell.clone()
                    } else {
                        format!("{cell:<w$}  ", w = widths[c])
                    }
                })
                .collect();
            if writeln!(stdout, "{}", line.trim_end()).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        let t = Tag::envelope("ask")
            .field("from", "boss/%0")
            .opt("bead", None)
            .opt("re", Some("x"))
            .text("hi");
        assert_eq!(t, "[hail kind:ask from:boss/%0 re:x] hi");
        assert_eq!(
            Tag::brief("fyi")
                .field("id", 1)
                .opt("scope", Some(""))
                .text("t"),
            "[hail fyi id:1] t"
        );
    }
}
