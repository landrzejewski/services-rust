//! JSON Web Tokens – issuing and verifying access tokens (step 019).
//!
//! A JWT is `base64url(header).base64url(claims).base64url(signature)`:
//! - header: `{"alg":"HS256","typ":"JWT"}`
//! - claims: JSON with who/what/when (`sub`, `exp`, ...) – readable by anyone (NOT encrypted!)
//! - signature: HMAC-SHA256(header.claims, secret) – proves integrity and origin.
//!
//! The server verifies the signature and the claims – no database lookup, no session storage
//! (stateless authentication).

use chrono::{TimeDelta, Utc};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode, errors::ErrorKind,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    config::AuthSettings,
    domain::user::{Role, User},
};

/// Payload of our access tokens.
/// Registered claims (RFC 7519): `sub`, `iss`, `aud`, `iat`, `exp`, `jti`; private: `email`, `role`.
/// Keep tokens small and free of sensitive data – anyone holding the token can read the claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject – the user id.
    pub sub: Uuid,
    pub email: String,
    pub role: String,
    pub iss: String,
    pub aud: String,
    /// Issued at / expiration time – seconds since the Unix epoch.
    pub iat: i64,
    pub exp: i64,
    /// Unique token id – enables revocation lists (denylist of `jti` until `exp`).
    pub jti: Uuid,
}

impl Claims {
    pub fn role(&self) -> Option<Role> {
        match self.role.as_str() {
            "USER" => Some(Role::User),
            "ADMIN" => Some(Role::Admin),
            _ => None,
        }
    }
}

/// Why a token was rejected – reported in `WWW-Authenticate: Bearer error="invalid_token"`.
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("token expired")]
    Expired,
    #[error("invalid token: {0}")]
    Invalid(String),
}

pub struct AccessToken {
    pub token: String,
    pub expires_in_seconds: i64,
}

pub struct JwtService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    validation: Validation,
    issuer: String,
    audience: String,
    ttl: TimeDelta,
}

impl JwtService {
    pub fn new(settings: &AuthSettings) -> anyhow::Result<Self> {
        let secret = settings.jwt_secret.expose_secret().as_bytes();
        // HS256 key should be at least as long as the hash output (256 bits).
        anyhow::ensure!(
            secret.len() >= 32,
            "auth.jwt_secret must have at least 32 bytes"
        );

        // Validation rules applied on decode. Pin the algorithm explicitly – accepting the
        // algorithm named in the token header enables "alg confusion" attacks (e.g. `none`).
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[&settings.jwt_issuer]);
        validation.set_audience(&[&settings.jwt_audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        // Tolerated clock difference between servers (seconds).
        validation.leeway = 30;

        Ok(Self {
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
            validation,
            issuer: settings.jwt_issuer.clone(),
            audience: settings.jwt_audience.clone(),
            ttl: TimeDelta::minutes(settings.access_token_ttl_minutes),
        })
    }

    pub fn issue(&self, user: &User) -> anyhow::Result<AccessToken> {
        let now = Utc::now();
        let claims = Claims {
            sub: user.id,
            email: user.email.to_string(),
            role: match user.role {
                Role::User => "USER".into(),
                Role::Admin => "ADMIN".into(),
            },
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            iat: now.timestamp(),
            exp: (now + self.ttl).timestamp(),
            jti: Uuid::now_v7(),
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding_key)?;
        Ok(AccessToken {
            token,
            expires_in_seconds: self.ttl.num_seconds(),
        })
    }

    /// Checks signature, `exp`, `iss`, `aud` and returns the claims.
    pub fn verify(&self, token: &str) -> Result<Claims, TokenError> {
        decode::<Claims>(token, &self.decoding_key, &self.validation)
            .map(|data| data.claims)
            .map_err(|error| match error.kind() {
                ErrorKind::ExpiredSignature => TokenError::Expired,
                _ => TokenError::Invalid(error.to_string()),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::user::Email;

    fn settings(ttl_minutes: i64) -> AuthSettings {
        AuthSettings {
            jwt_secret: "test-secret-with-at-least-32-bytes!!".into(),
            jwt_issuer: "test-issuer".into(),
            jwt_audience: "test-audience".into(),
            access_token_ttl_minutes: ttl_minutes,
        }
    }

    fn user() -> User {
        User {
            id: Uuid::now_v7(),
            email: Email::parse("anna@example.com").unwrap(),
            password_hash: None,
            role: Role::Admin,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn issued_token_is_verified() {
        let jwt = JwtService::new(&settings(15)).unwrap();
        let user = user();

        let token = jwt.issue(&user).unwrap();
        let claims = jwt.verify(&token.token).unwrap();

        assert_eq!(claims.sub, user.id);
        assert_eq!(claims.role(), Some(Role::Admin));
        assert_eq!(token.expires_in_seconds, 900);
    }

    #[test]
    fn expired_token_is_rejected() {
        // Negative TTL beyond the leeway -> already expired.
        let jwt = JwtService::new(&settings(-5)).unwrap();
        let token = jwt.issue(&user()).unwrap();

        assert!(matches!(jwt.verify(&token.token), Err(TokenError::Expired)));
    }

    #[test]
    fn tampered_token_is_rejected() {
        let jwt = JwtService::new(&settings(15)).unwrap();
        let token = jwt.issue(&user()).unwrap().token;
        // Change one character of the signature part.
        let mut tampered = token.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'A' { 'B' } else { 'A' });

        assert!(matches!(jwt.verify(&tampered), Err(TokenError::Invalid(_))));
    }

    #[test]
    fn token_for_another_audience_is_rejected() {
        let issuer = JwtService::new(&settings(15)).unwrap();
        let mut other = settings(15);
        other.jwt_audience = "another-api".into();
        let verifier = JwtService::new(&other).unwrap();

        let token = issuer.issue(&user()).unwrap();

        assert!(verifier.verify(&token.token).is_err());
    }
}
