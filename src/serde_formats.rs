//! Custom serde formats shared by the application.
//!
//! A module used with `#[serde(with = "module")]` must provide two functions:
//! - `serialize(&T, S) -> Result<S::Ok, S::Error>`
//! - `deserialize(D) -> Result<T, D::Error>`
//!
//! For one direction only use `serialize_with = "path"` / `deserialize_with = "path"`.

/// `NaiveTime` as `"HH:MM"` (e.g. `"08:30"`) instead of chrono's default `"08:30:00"`.
pub mod hh_mm {
    use chrono::NaiveTime;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    const FORMAT: &str = "%H:%M";

    pub fn serialize<S>(time: &NaiveTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // `collect_str` writes any `Display` value without allocating an intermediate String.
        serializer.collect_str(&time.format(FORMAT))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        // First deserialize into a type serde already knows, then convert.
        // `String` rather than `&str`: borrowing works only when the input contains the exact
        // bytes (no escape sequences) and the deserializer supports it – `String` always works.
        let text = String::deserialize(deserializer)?;
        // `D::Error::custom` turns any message into the deserializer's error type, so the
        // error surfaces as a normal JSON error (-> 422 with field path).
        NaiveTime::parse_from_str(&text, FORMAT)
            .map_err(|_| D::Error::custom(format!("invalid time `{text}`, expected HH:MM")))
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveTime;
    use serde::{Deserialize, Serialize};

    // Minimal struct using the custom format – tests the module in isolation.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Slot {
        #[serde(with = "super::hh_mm")]
        at: NaiveTime,
    }

    #[test]
    fn serializes_time_as_hh_mm() {
        let slot = Slot {
            at: NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
        };

        assert_eq!(serde_json::to_string(&slot).unwrap(), r#"{"at":"08:30"}"#);
    }

    #[test]
    fn round_trips() {
        let parsed: Slot = serde_json::from_str(r#"{"at":"17:45"}"#).unwrap();

        assert_eq!(parsed.at, NaiveTime::from_hms_opt(17, 45, 0).unwrap());
    }

    #[test]
    fn rejects_invalid_time_with_readable_message() {
        let error = serde_json::from_str::<Slot>(r#"{"at":"25:00"}"#).unwrap_err();

        assert!(error.to_string().contains("expected HH:MM"), "{error}");
    }
}
