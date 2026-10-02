//! Time source abstraction.

use chrono::{DateTime, Utc};

/// Business rules depend on "now" (booking must start in the future, cannot cancel a past booking).
/// Calling `Utc::now()` directly makes such rules untestable – tests would depend on the real
/// time. Injecting a `Clock` lets tests fix the time.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// Production implementation – the real system time.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Test implementation – always returns the same instant.
pub struct FixedClock(pub DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}
