//! Kinds, the one-line envelope and its `[hail …]` head, and headline
//! folding. Pure.

use std::fmt::Display;

/// What a message asks of its recipient. Control kinds are complete in the
/// envelope and never sit behind a fetch (DESIGN.md principle 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ruling,
    Go,
    Nogo,
    Ask,
    Fyi,
    Done,
    Stop,
    Hold,
    Block,
    Release,
    Announce,
}

impl Kind {
    pub const ALL: [Self; 11] = [
        Self::Ruling,
        Self::Go,
        Self::Nogo,
        Self::Ask,
        Self::Fyi,
        Self::Done,
        Self::Stop,
        Self::Hold,
        Self::Block,
        Self::Release,
        Self::Announce,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ruling => "ruling",
            Self::Go => "go",
            Self::Nogo => "nogo",
            Self::Ask => "ask",
            Self::Fyi => "fyi",
            Self::Done => "done",
            Self::Stop => "stop",
            Self::Hold => "hold",
            Self::Block => "block",
            Self::Release => "release",
            Self::Announce => "announce",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// `ruling go nogo …`, for error messages.
    pub fn list() -> String {
        Self::ALL.map(Self::as_str).join(" ")
    }

    /// Typed in full, no body, claimed (`inline`) at send.
    pub const fn is_control(self) -> bool {
        matches!(
            self,
            Self::Stop | Self::Hold | Self::Block | Self::Release | Self::Announce
        )
    }

    /// Leaves an obligation on the recipient until it sends `done --re`.
    pub const fn creates_obligation(self) -> bool {
        matches!(self, Self::Ruling | Self::Go | Self::Ask)
    }

    /// Closes or lifts another message, so `--re` is required.
    pub const fn needs_re(self) -> bool {
        matches!(self, Self::Done | Self::Release)
    }

    /// Sets a hold in effect until a `release`.
    pub const fn is_hold(self) -> bool {
        matches!(self, Self::Hold | Self::Block)
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Fields of the envelope head, in their printed order.
#[derive(Debug, Clone)]
pub struct Head<'a> {
    pub kind: Kind,
    pub from: &'a str,
    pub reply: &'a str,
    pub id: &'a str,
    /// The sub-agent a message is for; its parent relays it.
    pub for_: Option<&'a str>,
    pub re: Option<&'a str>,
    pub scope: Option<&'a str>,
    /// When a hold or block lapses.
    pub until: Option<&'a str>,
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

/// `[hail kind:<k> from:<f> reply:<r> id:<id> [for:] [re:] [scope:] [until:]] <headline>[ — hail inbox]`.
/// The fetch hint appears only when there is a body to fetch.
pub fn render(head: &Head<'_>, headline: &str, hint: bool) -> String {
    let line = Tag::envelope(head.kind)
        .field("from", head.from)
        .field("reply", head.reply)
        .field("id", head.id)
        .opt("for", head.for_)
        .opt("re", head.re)
        .opt("scope", head.scope)
        .opt("until", head.until)
        .text(headline);
    if hint {
        format!("{line} — hail inbox")
    } else {
        line
    }
}

/// A headline is typed into a terminal: tabs and newlines become spaces and
/// other control characters (ESC above all) are dropped. The body keeps every byte.
pub fn sanitize_headline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' | '\n' | '\r' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

/// Cut a long headline at the last sentence or clause boundary that fits the
/// cap, leaving room for the ellipsis; fall back to a word boundary, then a
/// hard cut. The caller carries the full text in the body. Counts characters.
pub fn fold_headline(text: &str, max: usize) -> String {
    let limit = max.saturating_sub(2);
    let head: String = text.chars().take(limit).collect();
    let mut cut = head.clone();
    for sep in [". ", "; ", ", ", " "] {
        if let Some(pos) = head.rfind(sep) {
            let before = &head[..pos];
            if before.chars().count() > limit / 2 {
                cut = before.to_string();
                break;
            }
        }
    }
    format!("{} …", cut.trim_end())
}

/// One brief line, at most `max` characters, with an ellipsis when cut.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        let t = Tag::envelope("ask")
            .field("from", "boss/%0")
            .opt("scope", None)
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
    use proptest::prelude::*;

    #[test]
    fn renders_the_envelope() {
        let head = Head {
            kind: Kind::Ruling,
            from: "api-1a/%5",
            reply: "api-1a",
            id: "0905T171200-a3f1",
            for_: None,
            until: None,
            re: None,
            scope: Some("commit"),
        };
        assert_eq!(
            render(&head, "convert at the receipt", true),
            "[hail kind:ruling from:api-1a/%5 reply:api-1a id:0905T171200-a3f1 scope:commit] convert at the receipt — hail inbox"
        );
    }

    #[test]
    fn sanitize_drops_escape_sequences() {
        assert_eq!(
            sanitize_headline("a\x1b[31mred\x1b[0m\tz\n"),
            "a[31mred[0m z"
        );
    }

    #[test]
    fn fold_prefers_a_sentence() {
        let t = "First sentence is here. Second sentence goes on and on and on and on.";
        assert_eq!(fold_headline(t, 40), "First sentence is here …");
    }

    proptest! {
        #[test]
        fn fold_fits_and_is_a_prefix(s in "\\PC{0,600}", max in 20usize..500) {
            let f = fold_headline(&s, max);
            prop_assert!(f.chars().count() <= max);
            let stem = f.strip_suffix(" …").unwrap_or(&f);
            prop_assert!(s.starts_with(stem));
        }
    }
}
