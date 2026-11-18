//! Dates and times, written the way the database and the app have always written them.
//!
//! Stored times and message keys hash [`rfc3339`]'s text, so its format must never change: a
//! different string for the same moment would count every old message again.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};

/// Today's date where the computer is.
pub fn today() -> Date {
    Zoned::now().date()
}

/// The date `ts` fell on where the computer is.
pub fn local_day(ts: Timestamp) -> Date {
    ts.to_zoned(TimeZone::system()).date()
}

/// The time zone the computer is in, to look up many times with [`Timestamp::to_zoned`].
pub fn local_zone() -> TimeZone {
    TimeZone::system()
}

/// Reads a time that names its offset, like `2026-10-20T10:00:00.123Z` or `…+02:00`.
pub fn parse(s: &str) -> Option<Timestamp> {
    s.parse().ok()
}

/// `2026-10-20T10:00:00.123+00:00`: UTC as `+00:00`, with 0, 3, 6 or 9 digits after the seconds,
/// whichever is the fewest that keeps every digit. What's stored, and what message keys hash.
pub fn rfc3339(ts: Timestamp) -> String {
    stamp(ts, Fraction::Fewest, "+00:00")
}

/// `2026-10-20T10:00:00.123Z`: always 3 digits, so stored times sort as text.
pub fn rfc3339_millis(ts: Timestamp) -> String {
    stamp(ts, Fraction::Millis, "Z")
}

/// Like [`rfc3339`] but ending in `Z`: what the app is sent.
pub fn rfc3339_z(ts: Timestamp) -> String {
    stamp(ts, Fraction::Fewest, "Z")
}

enum Fraction {
    Fewest,
    Millis,
}

fn stamp(ts: Timestamp, fraction: Fraction, utc: &str) -> String {
    let t = ts.to_zoned(TimeZone::UTC).datetime();
    let ns = t.subsec_nanosecond();
    let frac = match fraction {
        Fraction::Millis => format!(".{:03}", ns / 1_000_000),
        Fraction::Fewest if ns == 0 => String::new(),
        Fraction::Fewest if ns % 1_000_000 == 0 => format!(".{:03}", ns / 1_000_000),
        Fraction::Fewest if ns % 1_000 == 0 => format!(".{:06}", ns / 1_000),
        Fraction::Fewest => format!(".{ns:09}"),
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{frac}{utc}",
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    )
}

/// `#[serde(with = "sources::time::json")]` for an `Option<Timestamp>`, sent as [`rfc3339_z`].
pub mod json {
    use jiff::Timestamp;
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    /// Writes the time as [`super::rfc3339_z`], or null.
    pub fn serialize<S: Serializer>(ts: &Option<Timestamp>, s: S) -> Result<S::Ok, S::Error> {
        match ts {
            Some(ts) => s.serialize_str(&super::rfc3339_z(*ts)),
            None => s.serialize_none(),
        }
    }

    /// Reads a time with an offset, or null.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Timestamp>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|s| super::parse(&s).ok_or_else(|| D::Error::custom(format!("not a time: {s}"))))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn stored_times_keep_the_format_message_keys_were_built_from() {
        // These strings are what chrono wrote before the move to jiff. Changing them would change
        // every message key, and old messages would count again.
        let cases = [
            ("2026-10-20T10:00:00Z", "2026-10-20T10:00:00+00:00"),
            ("2026-10-20T10:00:00.5Z", "2026-10-20T10:00:00.500+00:00"),
            ("2026-10-20T10:00:00.123Z", "2026-10-20T10:00:00.123+00:00"),
            ("2026-10-20T10:00:00.1234Z", "2026-10-20T10:00:00.123400+00:00"),
            ("2026-10-20T10:00:00.123456Z", "2026-10-20T10:00:00.123456+00:00"),
            ("2026-10-20T10:00:00.1234567Z", "2026-10-20T10:00:00.123456700+00:00"),
            ("2026-10-20T10:00:00.000000001Z", "2026-10-20T10:00:00.000000001+00:00"),
            ("2026-10-20T12:30:00+02:30", "2026-10-20T10:00:00+00:00"),
            ("1969-12-31T23:59:59.5Z", "1969-12-31T23:59:59.500+00:00"),
        ];
        for (input, stored) in cases {
            assert_eq!(rfc3339(at(input)), stored, "{input}");
        }
        assert_eq!(rfc3339_millis(at("2026-10-20T10:00:00Z")), "2026-10-20T10:00:00.000Z");
        assert_eq!(rfc3339_millis(at("2026-10-20T10:00:00.9999Z")), "2026-10-20T10:00:00.999Z");
        assert_eq!(rfc3339_z(at("2026-10-20T10:00:00.25Z")), "2026-10-20T10:00:00.250Z");
    }

    #[test]
    fn times_without_an_offset_are_refused() {
        assert_eq!(parse("2026-10-20T10:00:00Z"), Some(at("2026-10-20T10:00:00Z")));
        assert_eq!(parse("2026-10-20T10:00:00"), None);
        assert_eq!(parse("yesterday"), None);
    }

    #[test]
    fn times_go_to_the_app_and_back() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct S {
            #[serde(with = "json")]
            t: Option<Timestamp>,
        }
        let s = S { t: Some(at("2026-10-20T10:00:00.5Z")) };
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(text, r#"{"t":"2026-10-20T10:00:00.500Z"}"#);
        assert_eq!(serde_json::from_str::<S>(&text).unwrap(), s);
        assert_eq!(serde_json::to_string(&S { t: None }).unwrap(), r#"{"t":null}"#);
        assert!(serde_json::from_str::<S>(r#"{"t":"soon"}"#).is_err());
    }
}
