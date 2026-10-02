//! Half-open time interval `[start, end)`.

use chrono::{DateTime, TimeDelta, Utc};

use crate::domain::validation::InvalidValue;

/// "Parse, don't validate": the only way to obtain a `TimeRange` is through `TimeRange::new`,
/// which checks the invariant `start < end`. Every function receiving a `TimeRange` can rely on
/// it being valid – no re-checking, no forgotten checks.
///
/// Fields are private, so code outside this module can't build an invalid value with
/// `TimeRange { start, end }` or mutate it afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

impl TimeRange {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Self, InvalidValue> {
        if start >= end {
            return Err(InvalidValue::new("endTime", "must be after startTime"));
        }
        Ok(Self { start, end })
    }

    // Getters return copies – `DateTime<Utc>` is `Copy`.
    pub fn start(&self) -> DateTime<Utc> {
        self.start
    }

    pub fn end(&self) -> DateTime<Utc> {
        self.end
    }

    pub fn duration(&self) -> TimeDelta {
        self.end - self.start
    }

    /// Two half-open ranges overlap when each starts before the other ends.
    /// `[9:00, 10:00)` and `[10:00, 11:00)` do NOT overlap – back-to-back bookings are allowed.
    pub fn overlaps(&self, other: &TimeRange) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 5, hour, 0, 0).unwrap()
    }

    #[test]
    fn rejects_empty_and_reversed_ranges() {
        assert!(TimeRange::new(at(10), at(10)).is_err());
        assert!(TimeRange::new(at(11), at(10)).is_err());
    }

    #[test]
    fn detects_overlaps() {
        let morning = TimeRange::new(at(9), at(11)).unwrap();

        assert!(morning.overlaps(&TimeRange::new(at(10), at(12)).unwrap()));
        assert!(morning.overlaps(&TimeRange::new(at(8), at(13)).unwrap()));
        assert!(!morning.overlaps(&TimeRange::new(at(11), at(12)).unwrap()));
    }
}
