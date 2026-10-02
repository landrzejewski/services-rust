use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::domain::{
    user::{Email, Password, Role, User},
    validation::InvalidValue,
};

/// Body of `POST /auth/register`.
// `SecretString` deserializes like a `String` (secrecy feature `serde`) but its `Debug` shows
// `[REDACTED]` – logging `?request` can't leak the password.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterRequest {
    #[validate(email(message = "must be a valid e-mail address"))]
    pub email: String,
    // No `#[validate]` here: validator's custom rules need the value to be `Serialize`, which
    // `SecretString` deliberately is not. The length policy is checked by `Password::parse`.
    pub password: SecretString,
}

impl RegisterRequest {
    pub fn into_domain(self) -> Result<(Email, Password), InvalidValue> {
        Ok((Email::parse(&self.email)?, Password::parse(self.password)?))
    }
}

/// Body of `POST /auth/login`. No format validation – any wrong input is just "invalid credentials".
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginRequest {
    pub email: String,
    pub password: SecretString,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoleDto {
    User,
    Admin,
}

impl From<Role> for RoleDto {
    fn from(role: Role) -> Self {
        match role {
            Role::User => Self::User,
            Role::Admin => Self::Admin,
        }
    }
}

/// Public representation of a user. Deliberately WITHOUT `password_hash` (step 008: DTOs
/// prevent leaking internal fields).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub role: RoleDto,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            email: user.email.to_string(),
            role: user.role.into(),
            created_at: user.created_at,
        }
    }
}

/// Response of `POST /auth/login` – shape follows the OAuth 2.0 token response (RFC 6749 §5.1).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: &'static str,
    /// Lifetime in seconds.
    pub expires_in: i64,
}
