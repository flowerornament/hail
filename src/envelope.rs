//! Kinds, the one-line envelope and its `[hail …]` head, and headline
//! folding. Pure.

use std::fmt::Display;

use crate::store::message::Message;

/// What a message asks of its recipient. Control kinds are complete in the
/// envelope and never sit behind a fetch (DESIGN.md principle 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ruling,
    Go,
    Nogo,
    Ask,
    Fyi,
    Stop,
    Hold,
    Block,
    Release,
    Announce,
}

impl Kind {
    pub const ALL: [Self; 10] = [
        Self::Ruling,
        Self::Go,
        Self::Nogo,
        Self::Ask,
        Self::Fyi,
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
            Self::Stop => "stop",
            Self::Hold => "hold",
            Self::Block => "block",
            Self::Release => "release",
            Self::Announce => "announce",
        }
    }

    /// Lenient: `None` for a kind this version does not know, such as the
    /// `done` older senders still write. Readers show such a message as it
    /// is stored; only a send refuses it ([`Kind::parse_sent`]).
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// A kind to send, or the usage text that says what to use instead.
    pub fn parse_sent(s: &str) -> Result<Self, String> {
        if s == "done" {
            return Err(
                "done is gone in 0.6: answer with any kind and --re <id> (fyi for an outcome)"
                    .into(),
            );
        }
        Self::parse(s).ok_or_else(|| format!("'{s}' is not a kind ({})", Self::list()))
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

    /// Lifts another message, so `--re` is required.
    pub const fn needs_re(self) -> bool {
        matches!(self, Self::Release)
    }

    /// Sets a hold in effect until a `release`.
    pub const fn is_hold(self) -> bool {
        matches!(self, Self::Hold | Self::Block)
    }

    /// Its sender waits for the answer: any message with `--re` to it.
    pub const fn asks_for_reply(self) -> bool {
        matches!(self, Self::Ruling | Self::Go | Self::Ask)
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

impl<'a> Head<'a> {
    /// The stored message: the head's fields as headers, `time:` after
    /// `id:`, and the headline as `ask:`. [`Head::from_message`] reads it
    /// back, so the typed envelope and the stored mail carry the same head.
    pub fn message(&self, time: &str, headline: &str, body: &str) -> Message {
        Message::default()
            .header("from", self.from)
            .header("reply", self.reply)
            .header("kind", self.kind.as_str())
            .header("id", self.id)
            .header("time", time)
            .header_opt("for", self.for_)
            .header_opt("re", self.re)
            .header_opt("scope", self.scope)
            .header_opt("until", self.until)
            .header("ask", headline)
            .with_body(body)
    }

    /// The head of a stored message; `None` when a required header is
    /// missing or the kind is unknown.
    pub fn from_message(m: &'a Message) -> Option<Self> {
        Some(Self {
            kind: Kind::parse(m.get("kind")?)?,
            from: m.get("from")?,
            reply: m.get("reply")?,
            id: m.get("id")?,
            for_: m.get("for"),
            re: m.get("re"),
            scope: m.get("scope"),
            until: m.get("until"),
        })
    }
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
    fn the_stored_message_reads_back_as_the_same_envelope() {
        let head = Head {
            kind: Kind::Ask,
            from: "web-1b/scout",
            reply: "web-1b/scout",
            id: "0905T171200-a3f1",
            for_: Some("scout"),
            re: Some("0905T170000-0001"),
            scope: None,
            until: Some("2026-09-06T01:12:00Z"),
        };
        let m = head.message("2026-09-05T17:12:00Z", "check the port", "the body");
        assert_eq!(
            m.render(),
            "from: web-1b/scout\nreply: web-1b/scout\nkind: ask\nid: 0905T171200-a3f1\ntime: 2026-09-05T17:12:00Z\nfor: scout\nre: 0905T170000-0001\nuntil: 2026-09-06T01:12:00Z\nask: check the port\n\nthe body\n"
        );
        let back = Head::from_message(&m).expect("a complete head");
        assert_eq!(
            render(&back, "check the port", false),
            render(&head, "check the port", false)
        );
        assert!(Head::from_message(&Message::default().header("kind", "ask")).is_none());
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

    #[test]
    fn done_is_refused_to_send_but_read_leniently() {
        let gone = Kind::parse_sent("done").unwrap_err();
        assert!(gone.contains("--re <id>"), "{gone}");
        assert!(!Kind::list().contains("done"));
        assert_eq!(Kind::parse("done"), None);
        assert_eq!(Kind::parse_sent("ask"), Ok(Kind::Ask));
        assert!(Kind::parse_sent("nope").unwrap_err().contains("not a kind"));
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
