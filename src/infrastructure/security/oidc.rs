//! Verification of access tokens issued by an external OpenID Connect provider (step 020).
//!
//! OpenID Connect discovery: the provider publishes metadata at
//! `{issuer}/.well-known/openid-configuration`, including `jwks_uri` – the URL of its PUBLIC
//! signing keys (JSON Web Key Set). Tokens are signed with the provider's private key (RS256);
//! we verify them with the matching public key, selected by the `kid` (key id) in the token header.
//! The provider can rotate keys at any time – unknown `kid` => download the JWKS again.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{config::OidcSettings, domain::user::Role, infrastructure::security::jwt::TokenError};

/// Claims we read from provider tokens (Keycloak layout).
#[derive(Debug, Deserialize)]
pub struct OidcClaims {
    /// Keycloak user ids are UUIDs.
    pub sub: Uuid,
    pub email: Option<String>,
    pub preferred_username: Option<String>,
    /// Keycloak puts realm roles under `realm_access.roles`. Other providers use other claims
    /// (`roles`, `groups`, `scope`) – role mapping is always provider-specific.
    #[serde(default)]
    pub realm_access: RealmAccess,
}

#[derive(Debug, Default, Deserialize)]
pub struct RealmAccess {
    #[serde(default)]
    pub roles: Vec<String>,
}

impl OidcClaims {
    pub fn role(&self) -> Role {
        if self.realm_access.roles.iter().any(|role| role == "ADMIN") {
            Role::Admin
        } else {
            Role::User
        }
    }
}

#[derive(Deserialize)]
struct DiscoveryDocument {
    issuer: String,
    jwks_uri: String,
}

pub struct OidcVerifier {
    settings: OidcSettings,
    http: reqwest::Client,
    validation: Validation,
    /// `kid` -> public key. `tokio::sync::RwLock`: many concurrent readers, rare writers.
    keys: RwLock<HashMap<String, DecodingKey>>,
    /// Time of the last JWKS download; the `Mutex` also ensures only one task downloads at a time.
    last_refresh: Mutex<Option<Instant>>,
}

impl OidcVerifier {
    /// Doesn't contact the provider – keys are fetched lazily on the first token. The service
    /// starts even when the identity provider is temporarily down.
    pub fn new(settings: OidcSettings) -> anyhow::Result<Self> {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&settings.issuer]);
        validation.set_audience(&[&settings.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 30;

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        Ok(Self {
            settings,
            http,
            validation,
            keys: RwLock::new(HashMap::new()),
            last_refresh: Mutex::new(None),
        })
    }

    pub async fn verify(&self, token: &str) -> Result<OidcClaims, TokenError> {
        // The header is NOT trusted for the algorithm (pinned to RS256 in `validation`);
        // it is only used to pick the key.
        let header = decode_header(token).map_err(|e| TokenError::Invalid(e.to_string()))?;
        let kid = header
            .kid
            .ok_or_else(|| TokenError::Invalid("token without kid".into()))?;

        let key = self.key(&kid).await?;
        decode::<OidcClaims>(token, &key, &self.validation)
            .map(|data| data.claims)
            .map_err(|error| match error.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => TokenError::Expired,
                _ => TokenError::Invalid(error.to_string()),
            })
    }

    async fn key(&self, kid: &str) -> Result<DecodingKey, TokenError> {
        if let Some(key) = self.keys.read().await.get(kid) {
            return Ok(key.clone());
        }
        // Unknown key: first use or the provider rotated keys -> refresh (rate-limited).
        self.refresh_keys().await?;
        self.keys
            .read()
            .await
            .get(kid)
            .cloned()
            .ok_or_else(|| TokenError::Invalid(format!("unknown signing key {kid}")))
    }

    async fn refresh_keys(&self) -> Result<(), TokenError> {
        let mut last_refresh = self.last_refresh.lock().await;
        let min_interval = Duration::from_secs(self.settings.jwks_min_refresh_secs);
        if last_refresh.is_some_and(|at| at.elapsed() < min_interval) {
            // Someone refreshed recently – don't let random `kid`s hammer the provider.
            return Ok(());
        }
        *last_refresh = Some(Instant::now());

        let keys = self.download_keys().await.map_err(|error| {
            tracing::warn!(%error, "fetching OIDC signing keys failed");
            TokenError::Invalid("identity provider keys unavailable".into())
        })?;
        tracing::info!(count = keys.len(), "OIDC signing keys loaded");
        *self.keys.write().await = keys;
        Ok(())
    }

    async fn download_keys(&self) -> anyhow::Result<HashMap<String, DecodingKey>> {
        let discovery_url = self.settings.discovery_url.clone().unwrap_or_else(|| {
            format!("{}/.well-known/openid-configuration", self.settings.issuer)
        });
        let discovery: DiscoveryDocument = self
            .http
            .get(&discovery_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        anyhow::ensure!(
            discovery.issuer == self.settings.issuer,
            "issuer mismatch: provider says {}, configured {}",
            discovery.issuer,
            self.settings.issuer
        );

        let jwks: JwkSet = self
            .http
            .get(&discovery.jwks_uri)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // Keep only signing keys we can use; ignore encryption keys and unsupported types.
        Ok(jwks
            .keys
            .iter()
            .filter_map(|jwk| {
                let kid = jwk.common.key_id.clone()?;
                let key = DecodingKey::from_jwk(jwk).ok()?;
                Some((kid, key))
            })
            .collect())
    }
}
