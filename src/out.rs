//! Output shared by every verb: stdout that ends quietly on a closed pipe,
//! and aligned tables. The `[hail …]` head of a line is `envelope::Tag`.

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
