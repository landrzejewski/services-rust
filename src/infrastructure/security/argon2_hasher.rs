//! Argon2id password hashing.
//!
//! Why Argon2id: winner of the Password Hashing Competition, recommended by OWASP. It is
//! *memory-hard* – each guess needs ~19 MiB of RAM, which makes GPU/ASIC brute force expensive.
//! Never use fast hashes (SHA-256, MD5) for passwords – they allow billions of guesses per second.

use argon2::{
    Argon2,
    password_hash::{PasswordHasher as _, PasswordVerifier, phc::PasswordHash},
};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};

use crate::domain::password::{PasswordHashError, PasswordHasher};

pub struct Argon2PasswordHasher {
    /// Hash of a random value, used by `verify_dummy` to equalize response times.
    dummy_hash: String,
}

impl Argon2PasswordHasher {
    pub fn new() -> Result<Self, PasswordHashError> {
        let dummy_hash = hash_blocking(&SecretString::from(uuid::Uuid::now_v7().to_string()))?;
        Ok(Self { dummy_hash })
    }
}

// Default parameters = Argon2id v19, m=19456 KiB, t=2, p=1 (OWASP minimum recommendation).
// A random salt is generated per hash; salt and parameters are stored inside the PHC string:
//   $argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>
// so parameters can be raised later without breaking existing hashes.
fn hash_blocking(password: &SecretString) -> Result<String, PasswordHashError> {
    Argon2::default()
        .hash_password(password.expose_secret().as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|e| PasswordHashError(e.to_string()))
}

fn verify_blocking(password: &SecretString, hash: &str) -> Result<bool, PasswordHashError> {
    let parsed = PasswordHash::new(hash).map_err(|e| PasswordHashError(e.to_string()))?;
    // Parameters and salt are read from the stored hash; comparison is constant-time.
    Ok(Argon2::default()
        .verify_password(password.expose_secret().as_bytes(), &parsed)
        .is_ok())
}

#[async_trait]
impl PasswordHasher for Argon2PasswordHasher {
    async fn hash(&self, password: &SecretString) -> Result<String, PasswordHashError> {
        // Argon2 deliberately burns tens of milliseconds of CPU – too long for an async worker
        // thread (step 002). `spawn_blocking` moves it to the blocking thread pool.
        // The closure must own its data (`'static`), hence the clone of the secret.
        let password = password.clone();
        tokio::task::spawn_blocking(move || hash_blocking(&password))
            .await
            .map_err(|e| PasswordHashError(e.to_string()))?
    }

    async fn verify(&self, password: &SecretString, hash: &str) -> Result<bool, PasswordHashError> {
        let password = password.clone();
        let hash = hash.to_string();
        tokio::task::spawn_blocking(move || verify_blocking(&password, &hash))
            .await
            .map_err(|e| PasswordHashError(e.to_string()))?
    }

    async fn verify_dummy(&self, password: &SecretString) {
        // Result ignored on purpose – only the time spent matters.
        let _ = self.verify(password, &self.dummy_hash).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hashes_are_salted_and_verifiable() {
        let hasher = Argon2PasswordHasher::new().unwrap();
        let password = SecretString::from("correct horse battery staple");

        let first = hasher.hash(&password).await.unwrap();
        let second = hasher.hash(&password).await.unwrap();

        // Same password, different random salts -> different hashes.
        assert_ne!(first, second);
        assert!(first.starts_with("$argon2id$"));
        assert!(hasher.verify(&password, &first).await.unwrap());
        assert!(
            !hasher
                .verify(&SecretString::from("wrong password!"), &first)
                .await
                .unwrap()
        );
    }
}
