use std::fmt;

use reqwest::{Method, RequestBuilder};
use serde::{Deserialize, Serialize};

use crate::{Client, Error, Result};

/// A DataStore selected by an API-key scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyDataStore {
    /// The universe identifier.
    pub universe_id: String,
    /// The DataStore name.
    pub datastore_name: String,
}

/// One scope granted to an API key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyScope {
    /// The scope name.
    pub name: String,
    /// Operations granted within the scope.
    #[serde(default)]
    pub operations: Vec<String>,
    /// Selected user identifiers, including the `*` wildcard.
    #[serde(default)]
    pub user_ids: Vec<String>,
    /// Selected group identifiers, including the `*` wildcard.
    #[serde(default)]
    pub group_ids: Vec<String>,
    /// Selected universe identifiers, including the `*` wildcard.
    #[serde(default)]
    pub universe_ids: Vec<String>,
    /// Selected universe and DataStore pairs.
    #[serde(default)]
    pub universe_datastores: Vec<ApiKeyDataStore>,
}

/// Information returned by API-key introspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyInfo {
    /// The key's display name.
    pub name: String,
    /// The user that last generated the key.
    #[serde(default)]
    pub authorized_user_id: Option<u64>,
    /// Scopes granted to the key.
    #[serde(default)]
    pub scopes: Vec<ApiKeyScope>,
    /// Whether the key is enabled.
    pub enabled: bool,
    /// Whether the key has expired.
    pub expired: bool,
    /// The configured expiration time.
    #[serde(default)]
    pub expiration_time_utc: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyIntrospectionRequest<'key> {
    api_key: &'key str,
}

/// A supported Open Cloud authentication mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authentication {
    /// Authentication through the `x-api-key` header.
    ApiKey,
    /// Authentication through an OAuth bearer access token.
    OAuth,
}

impl fmt::Display for Authentication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey => formatter.write_str("API-key authentication"),
            Self::OAuth => formatter.write_str("OAuth authentication"),
        }
    }
}

/// Credentials attached to Open Cloud requests.
#[derive(Clone)]
pub struct Credentials {
    authentication: Authentication,
    secret: String,
}

impl Credentials {
    /// Creates API-key credentials.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCredential`] when the key is empty.
    pub fn api_key(key: impl Into<String>) -> Result<Self> {
        Self::new(Authentication::ApiKey, key.into(), "API key")
    }

    /// Creates OAuth bearer-token credentials.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCredential`] when the token is empty.
    pub fn oauth(token: impl Into<String>) -> Result<Self> {
        Self::new(Authentication::OAuth, token.into(), "OAuth access token")
    }

    /// Returns the credential's authentication mode.
    #[must_use]
    pub const fn authentication(&self) -> Authentication {
        self.authentication
    }

    fn new(authentication: Authentication, secret: String, kind: &'static str) -> Result<Self> {
        if secret.is_empty() {
            return Err(Error::InvalidCredential { kind });
        }

        Ok(Self {
            authentication,
            secret,
        })
    }

    pub(crate) fn authorize(&self, builder: RequestBuilder) -> RequestBuilder {
        match self.authentication {
            Authentication::ApiKey => builder.header("x-api-key", &self.secret),
            Authentication::OAuth => builder.bearer_auth(&self.secret),
        }
    }

    pub(crate) fn secret(&self) -> &str {
        &self.secret
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credentials")
            .field("authentication", &self.authentication)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

impl Client {
    /// Retrieves information about this client's API key.
    ///
    /// # Errors
    ///
    /// Returns an error when this is an OAuth client, the request fails, Roblox
    /// rejects it, or the response is invalid.
    pub async fn introspect_api_key(&self) -> Result<ApiKeyInfo> {
        if self.authentication() != Authentication::ApiKey {
            return Err(Error::UnsupportedAuthentication {
                operation: "introspect API key",
                authentication: self.authentication(),
            });
        }

        let body = ApiKeyIntrospectionRequest {
            api_key: self.credentials().secret(),
        };
        let builder = self
            .request_with_authentication(Method::POST, "/api-keys/v1/introspect", false)?
            .json(&body);
        self.send_json(builder).await
    }
}
