//! Every time format hail writes, in one place.

use std::time::SystemTime;

use jiff::Timestamp;

pub fn now() -> Timestamp {
    Timestamp::now()
}

/// `2026-10-06T19:00:46Z`: message `time:` headers and receipts.
pub fn iso(ts: Timestamp) -> String {
    ts.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// `1006T190046`: the time part of a message id. No year, kept from 0.3 because
/// agents copy and type ids; uniqueness comes from the id index (`store::ids`).
pub fn id_stamp(ts: Timestamp) -> String {
    ts.strftime("%m%dT%H%M%S").to_string()
}

pub fn from_system(t: SystemTime) -> Timestamp {
    Timestamp::try_from(t).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// `30m`, `8h`, `3d`: a length of time as agents type it.
pub fn parse_span(s: &str) -> Option<std::time::Duration> {
    let (n, unit) = s.trim().split_at(s.trim().len().checked_sub(1)?);
    let n: u64 = n.parse().ok().filter(|n| *n > 0)?;
    let secs = match unit {
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return None,
    };
    Some(std::time::Duration::from_secs(n.checked_mul(secs)?))
}

/// `ts` moved forward by `d`, saturating rather than failing.
pub fn after(ts: Timestamp, d: std::time::Duration) -> Timestamp {
    jiff::SignedDuration::try_from(d)
        .ok()
        .and_then(|d| ts.checked_add(d).ok())
        .unwrap_or(Timestamp::MAX)
}

/// `ts` moved back by `d`, saturating rather than failing.
pub fn before(ts: Timestamp, d: std::time::Duration) -> Timestamp {
    jiff::SignedDuration::try_from(d)
        .ok()
        .and_then(|d| ts.checked_sub(d).ok())
        .unwrap_or(Timestamp::MIN)
}

pub fn parse_iso(s: &str) -> Option<Timestamp> {
    s.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_round_trip() {
        let ts: Timestamp = "2026-10-06T19:00:46Z".parse().unwrap();
        assert_eq!(iso(ts), "2026-10-06T19:00:46Z");
        assert_eq!(id_stamp(ts), "1006T190046");
        assert_eq!(parse_iso("2026-10-06T19:00:46Z"), Some(ts));
    }

    #[test]
    fn spans() {
        use std::time::Duration;
        assert_eq!(parse_span("30m"), Some(Duration::from_mins(30)));
        assert_eq!(parse_span("8h"), Some(Duration::from_hours(8)));
        assert_eq!(parse_span("3d"), Some(Duration::from_hours(72)));
        for bad in ["", "h", "0h", "8", "8x", "-1h", "1.5h"] {
            assert_eq!(parse_span(bad), None, "{bad}");
        }
    }
}
