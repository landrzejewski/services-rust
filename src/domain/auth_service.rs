//! Registration and authentication (step 018).

use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
};

use secrecy::SecretString;
use uuid::Uuid;

use crate::domain::{
    error::{DomainError, DomainResult},
    password::PasswordHasher,
    repositories::UserRepository,
    user::{Email, NewUser, Password, Role, User},
};

pub struct AuthService {
    users: Arc<dyn UserRepository>,
    hasher: Arc<dyn PasswordHasher>,
    /// External users already provisioned by this process (avoids a DB write per request).
    provisioned: RwLock<HashSet<Uuid>>,
}

impl AuthService {
    pub fn new(users: Arc<dyn UserRepository>, hasher: Arc<dyn PasswordHasher>) -> Self {
        Self {
            users,
            hasher,
            provisioned: RwLock::new(HashSet::new()),
        }
    }

    /// Just-in-time provisioning (step 020): users authenticated by the identity provider get a
    /// local `users` row on their first request, so bookings can reference them (foreign key).
    /// The provider stays the source of truth for e-mail and role.
    pub async fn ensure_external_user(
        &self,
        id: Uuid,
        email: &str,
        role: Role,
    ) -> DomainResult<()> {
        if self
            .provisioned
            .read()
            .expect("provisioned lock poisoned")
            .contains(&id)
        {
            return Ok(());
        }
        let email = Email::parse(email).map_err(|_| DomainError::Unauthenticated)?;
        self.users.upsert_external(id, &email, role).await?;
        self.provisioned
            .write()
            .expect("provisioned lock poisoned")
            .insert(id);
        Ok(())
    }

    /// Self-registration always creates a regular `USER` – a client can't choose its role.
    pub async fn register(&self, email: Email, password: Password) -> DomainResult<User> {
        let password_hash = self
            .hasher
            .hash(password.secret())
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;
        // A duplicate e-mail violates the unique index -> `Conflict` (409).
        // (Revealing "already registered" is a deliberate UX trade-off; stricter systems
        // always answer "check your inbox" and send an e-mail instead.)
        Ok(self
            .users
            .insert(NewUser {
                email,
                password_hash: Some(password_hash),
                role: Role::User,
            })
            .await?)
    }

    /// Verifies credentials. Every failure – unknown e-mail, wrong password, account without a
    /// local password – produces the SAME error, so an attacker can't tell which part was wrong.
    pub async fn authenticate(&self, email: &str, password: &SecretString) -> DomainResult<User> {
        let user = match Email::parse(email) {
            Ok(email) => self.users.find_by_email(&email).await?,
            Err(_) => None,
        };

        let Some(user) = user else {
            self.hasher.verify_dummy(password).await;
            return Err(DomainError::Unauthenticated);
        };
        let Some(hash) = user.password_hash.as_deref() else {
            self.hasher.verify_dummy(password).await;
            return Err(DomainError::Unauthenticated);
        };

        let valid = self
            .hasher
            .verify(password, hash)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;
        if valid {
            Ok(user)
        } else {
            Err(DomainError::Unauthenticated)
        }
    }

    pub async fn get_user(&self, id: Uuid) -> DomainResult<User> {
        self.users
            .find_by_id(id)
            .await?
            .ok_or(DomainError::NotFound { entity: "user", id })
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use secrecy::ExposeSecret;

    use super::*;
    use crate::{
        domain::password::PasswordHashError, infrastructure::memory::InMemoryUserRepository,
    };

    // Fast fake hasher – Argon2 itself is tested in infrastructure; here we test the service logic.
    struct PlainHasher;

    #[async_trait]
    impl PasswordHasher for PlainHasher {
        async fn hash(&self, password: &SecretString) -> Result<String, PasswordHashError> {
            Ok(format!("plain:{}", password.expose_secret()))
        }
        async fn verify(
            &self,
            password: &SecretString,
            hash: &str,
        ) -> Result<bool, PasswordHashError> {
            Ok(hash == format!("plain:{}", password.expose_secret()))
        }
        async fn verify_dummy(&self, _: &SecretString) {}
    }

    fn service() -> AuthService {
        AuthService::new(
            Arc::new(InMemoryUserRepository::default()),
            Arc::new(PlainHasher),
        )
    }

    fn secret(value: &str) -> SecretString {
        SecretString::from(value)
    }

    async fn register(service: &AuthService, email: &str) -> User {
        service
            .register(
                Email::parse(email).unwrap(),
                Password::parse(secret("long enough password")).unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn registers_regular_user_with_hashed_password() {
        let service = service();

        let user = register(&service, "Anna@Example.com").await;

        assert_eq!(user.email.as_str(), "anna@example.com");
        assert_eq!(user.role, Role::User);
        assert_ne!(user.password_hash.as_deref(), Some("long enough password"));
    }

    #[tokio::test]
    async fn authenticates_with_correct_password_case_insensitive_email() {
        let service = service();
        register(&service, "anna@example.com").await;

        let user = service
            .authenticate("ANNA@example.com", &secret("long enough password"))
            .await;

        assert!(user.is_ok());
    }

    #[tokio::test]
    async fn wrong_password_and_unknown_user_give_the_same_error() {
        let service = service();
        register(&service, "anna@example.com").await;

        let wrong_password = service
            .authenticate("anna@example.com", &secret("bad password!!"))
            .await;
        let unknown_user = service
            .authenticate("nobody@example.com", &secret("long enough password"))
            .await;

        assert!(matches!(wrong_password, Err(DomainError::Unauthenticated)));
        assert!(matches!(unknown_user, Err(DomainError::Unauthenticated)));
    }

    #[tokio::test]
    async fn duplicate_email_is_a_conflict() {
        let service = service();
        register(&service, "anna@example.com").await;

        let duplicate = service
            .register(
                Email::parse("ANNA@example.com").unwrap(),
                Password::parse(secret("another password")).unwrap(),
            )
            .await;

        assert!(matches!(duplicate, Err(DomainError::Conflict(_))));
    }
}
