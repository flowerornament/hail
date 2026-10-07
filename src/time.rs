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
/// agents copy and type ids; uniqueness comes from the id index (store::ids).
pub fn id_stamp(ts: Timestamp) -> String {
    ts.strftime("%m%dT%H%M%S").to_string()
}

pub fn from_system(t: SystemTime) -> Timestamp {
    Timestamp::try_from(t).unwrap_or(Timestamp::UNIX_EPOCH)
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
}
