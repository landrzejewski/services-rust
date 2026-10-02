//! Password hashing port (step 018).

use async_trait::async_trait;
use secrecy::SecretString;

/// Hashing failed for a technical reason (invalid stored hash format, internal error).
#[derive(Debug, thiserror::Error)]
#[error("password hashing failed: {0}")]
pub struct PasswordHashError(pub String);

/// Passwords are never stored or compared directly – only slow, salted hashes.
/// The domain depends on this trait; the Argon2 implementation lives in infrastructure.
#[async_trait]
pub trait PasswordHasher: Send + Sync {
    /// Returns a self-describing hash string (algorithm, parameters, salt, hash).
    async fn hash(&self, password: &SecretString) -> Result<String, PasswordHashError>;

    /// Constant-time comparison of the password with a stored hash.
    async fn verify(&self, password: &SecretString, hash: &str) -> Result<bool, PasswordHashError>;

    /// Spends the same time as `verify` without a real hash. Called when the user does not
    /// exist, so response times don't reveal which e-mails are registered (user enumeration).
    async fn verify_dummy(&self, password: &SecretString);
}
