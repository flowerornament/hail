//! The message and record file format: `key: value` header lines, a blank
//! line, then the body. Agents and humans `cat` these, and the 0.3 import and
//! revert read and write it, so keep it stable.

use std::fmt::Write as _;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Message {
    pub fn header(mut self, key: &str, value: impl Into<String>) -> Self {
        self.headers.push((key.to_string(), value.into()));
        self
    }

    pub fn header_opt(self, key: &str, value: Option<&str>) -> Self {
        match value {
            Some(v) => self.header(key, v),
            None => self,
        }
    }

    #[must_use]
    pub fn with_body(mut self, body: &str) -> Self {
        self.body = body.to_string();
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The text of the message: headers, a blank line, the body and a newline.
    pub fn render(&self) -> String {
        let mut s = String::new();
        for (k, v) in &self.headers {
            s.push_str(k);
            s.push_str(": ");
            s.push_str(v);
            s.push('\n');
        }
        s.push('\n');
        s.push_str(&self.body);
        s.push('\n');
        s
    }

    /// A record (an obligation, hold or pending send) is headers only.
    pub fn render_record(&self) -> String {
        self.headers.iter().fold(String::new(), |mut s, (k, v)| {
            let _ = writeln!(s, "{k}: {v}");
            s
        })
    }

    /// Lenient: header lines run until the first blank line; a line without
    /// `: ` ends the headers too. The body is the rest, minus one trailing newline.
    pub fn parse(text: &str) -> Self {
        let mut headers = Vec::new();
        let mut rest = text;
        loop {
            let (line, tail) = match rest.split_once('\n') {
                Some((l, t)) => (l, t),
                None => (rest, ""),
            };
            if line.is_empty() {
                rest = tail;
                break;
            }
            match line.split_once(": ") {
                Some((k, v)) if !k.contains(' ') => headers.push((k.to_string(), v.to_string())),
                _ => break,
            }
            rest = tail;
            if rest.is_empty() {
                break;
            }
        }
        let body = rest.strip_suffix('\n').unwrap_or(rest).to_string();
        Self { headers, body }
    }

    /// True when the body is just the headline, so a typed envelope already
    /// carried all of it.
    pub fn body_is_headline(&self) -> bool {
        self.get("ask") == Some(self.body.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parses_a_0_3_message() {
        let text = "from: boss/%0\nreply: %0\nkind: ruling\nid: 0905T1712-a3f1\nask: do it\n\nline one\nline two\n";
        let m = Message::parse(text);
        assert_eq!(m.get("kind"), Some("ruling"));
        assert_eq!(m.get("ask"), Some("do it"));
        assert_eq!(m.body, "line one\nline two");
        assert_eq!(m.render(), text);
    }

    proptest! {
        #[test]
        fn round_trips_any_body(body in "\\PC*", ask in "[^\n]{0,80}") {
            let m = Message::default().header("kind", "fyi").header("ask", ask).with_body(&body);
            prop_assert_eq!(Message::parse(&m.render()), m);
        }
    }
}
