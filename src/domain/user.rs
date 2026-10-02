//! Users and roles (step 018).

use std::fmt;

use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::domain::validation::InvalidValue;

#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub email: Email,
    /// Argon2 PHC string. `None` for users of an external identity provider (step 020).
    /// Never leaves the domain/infrastructure – `UserResponse` has no such field.
    pub password_hash: Option<String>,
    pub role: Role,
    pub created_at: DateTime<Utc>,
}

/// Authorization role (used for access decisions in step 021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Admin,
}

/// Who performs an operation – the domain's view of the authenticated caller (step 021).
/// Services use it for *resource-based* authorization ("is this the owner?"), which the API layer
/// can't decide without loading the resource.
#[derive(Debug, Clone, Copy)]
pub struct Actor {
    pub id: Uuid,
    pub role: Role,
}

impl Actor {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }

    /// Owner or admin.
    pub fn can_manage(&self, owner_id: Uuid) -> bool {
        self.is_admin() || self.id == owner_id
    }
}

#[derive(Debug, Clone)]
pub struct NewUser {
    pub email: Email,
    pub password_hash: Option<String>,
    pub role: Role,
}

/// Normalized (trimmed, lowercase) e-mail address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email(String);

impl Email {
    pub fn parse(value: &str) -> Result<Self, InvalidValue> {
        let normalized = value.trim().to_lowercase();
        // Deliberately simple: full RFC 5322 validation is impractical; the only real proof
        // of an address is a confirmation e-mail.
        let valid = normalized.len() <= 254
            && !normalized.contains(char::is_whitespace)
            && normalized
                .split_once('@')
                .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'));
        if !valid {
            return Err(InvalidValue::new("email", "must be a valid e-mail address"));
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Email {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A password that satisfies the password policy.
///
/// Wraps `SecretString` (crate `secrecy`): its `Debug` prints `[REDACTED]`, so the password
/// can't end up in logs by accident (`?request`, panics, error messages), and the memory is
/// zeroed when dropped. Reading it requires an explicit `expose_secret()`.
#[derive(Clone)]
pub struct Password(SecretString);

impl Password {
    pub const MIN_LENGTH: usize = 12;
    pub const MAX_LENGTH: usize = 128;

    /// Policy (NIST SP 800-63B style): length matters most; no composition rules
    /// ("one digit, one special char") – they don't improve real-world strength.
    pub fn parse(value: SecretString) -> Result<Self, InvalidValue> {
        let length = value.expose_secret().chars().count();
        if !(Self::MIN_LENGTH..=Self::MAX_LENGTH).contains(&length) {
            return Err(InvalidValue::new(
                "password",
                format!(
                    "must have {}-{} characters",
                    Self::MIN_LENGTH,
                    Self::MAX_LENGTH
                ),
            ));
        }
        Ok(Self(value))
    }

    pub fn secret(&self) -> &SecretString {
        &self.0
    }
}
