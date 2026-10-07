//! Kinds, the one-line envelope, headline folding and bead detection. Pure.

pub const KINDS: [&str; 11] = [
    "ruling", "go", "nogo", "ask", "fyi", "done", "stop", "hold", "block", "release", "announce",
];

pub fn kinds_list() -> String {
    KINDS.join(" ")
}

pub fn is_kind(k: &str) -> bool {
    KINDS.contains(&k)
}

/// Control kinds are complete in the envelope and never sit behind a fetch
/// (DESIGN.md principle 5): typed in full, no body, receipt pre-written.
pub fn is_control(k: &str) -> bool {
    matches!(k, "stop" | "hold" | "block" | "release" | "announce")
}

/// Kinds that leave an obligation on the recipient until it sends `done --re`.
pub fn creates_obligation(k: &str) -> bool {
    matches!(k, "ruling" | "go" | "ask")
}

/// Fields of the envelope head, in their printed order.
#[derive(Debug, Default, Clone)]
pub struct Head<'a> {
    pub kind: &'a str,
    pub from: &'a str,
    pub reply: &'a str,
    pub id: &'a str,
    pub bead: Option<&'a str>,
    pub re: Option<&'a str>,
    pub scope: Option<&'a str>,
}

/// `[hail kind:<k> from:<f> reply:<r> id:<id> [bead:] [re:] [scope:]] <headline>[ — hail inbox]`.
/// The fetch hint appears only when there is a body to fetch.
pub fn render(head: &Head, headline: &str, hint: bool) -> String {
    let mut s = format!(
        "[hail kind:{} from:{} reply:{} id:{}",
        head.kind, head.from, head.reply, head.id
    );
    for (k, v) in [("bead", head.bead), ("re", head.re), ("scope", head.scope)] {
        if let Some(v) = v {
            s.push_str(&format!(" {k}:{v}"));
        }
    }
    s.push_str("] ");
    s.push_str(headline);
    if hint {
        s.push_str(" — hail inbox");
    }
    s
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

/// First token that looks like a beads issue id (`prefix-hash[.n]`). The hash
/// must contain a digit so ordinary hyphenated words (tmux-bridge, read-only)
/// are not mistaken for an issue.
pub fn detect_bead(text: &str) -> Option<String> {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                ' '
            }
        })
        .collect();
    cleaned.split_whitespace().find_map(|tok| {
        let tok = tok.strip_suffix('.').unwrap_or(tok);
        is_bead_id(tok).then(|| tok.to_string())
    })
}

fn is_bead_id(tok: &str) -> bool {
    let Some((prefix, rest)) = tok.split_once('-') else {
        return false;
    };
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_lowercase()) {
        return false;
    }
    let (hash, child) = match rest.split_once('.') {
        Some((h, n)) => (h, Some(n)),
        None => (rest, None),
    };
    let hash_ok = (4..=6).contains(&hash.len())
        && hash
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && hash.chars().any(|c| c.is_ascii_digit());
    let child_ok = child.is_none_or(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    hash_ok && child_ok
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
    use proptest::prelude::*;

    #[test]
    fn renders_the_envelope() {
        let head = Head {
            kind: "ruling",
            from: "murail-1a/%5",
            reply: "murail-1a",
            id: "0905T171200-a3f1",
            bead: Some("murail-ke7is"),
            re: None,
            scope: Some("commit"),
        };
        assert_eq!(
            render(&head, "convert at the receipt", true),
            "[hail kind:ruling from:murail-1a/%5 reply:murail-1a id:0905T171200-a3f1 bead:murail-ke7is scope:commit] convert at the receipt — hail inbox"
        );
    }

    #[test]
    fn beads_need_a_digit() {
        assert_eq!(
            detect_bead("see murail-ke7is now"),
            Some("murail-ke7is".into())
        );
        assert_eq!(detect_bead("ends herald-abc.2."), None);
        assert_eq!(
            detect_bead("x herald-ab12.2."),
            Some("herald-ab12.2".into())
        );
        assert_eq!(detect_bead("tmux-bridge read-only"), None);
        assert_eq!(detect_bead("(murail-9sg8y)"), Some("murail-9sg8y".into()));
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
