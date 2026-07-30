//! OAuth 2.0 authorization-code flow with PKCE.

use std::collections::BTreeMap;
use std::fmt;

use oauth2::basic::BasicClient;
use oauth2::{
    AuthType, AuthUrl, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenUrl,
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::check_status;
use crate::{Error, Result};

const AUTH_URL: &str = "https://apis.roblox.com/oauth/v1/authorize";
const TOKEN_URL: &str = "https://apis.roblox.com/oauth/v1/token";
const INTROSPECTION_URL: &str = "https://apis.roblox.com/oauth/v1/token/introspect";
const RESOURCES_URL: &str = "https://apis.roblox.com/oauth/v1/token/resources";
const REVOKE_URL: &str = "https://apis.roblox.com/oauth/v1/token/revoke";
const USER_INFO_URL: &str = "https://apis.roblox.com/oauth/v1/userinfo";
const DISCOVERY_URL: &str = "https://apis.roblox.com/oauth/.well-known/openid-configuration";

type InnerClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// A pending OAuth authorization.
///
/// Keep this value until Roblox redirects back to the application. It contains
/// the PKCE verifier needed to exchange the returned code.
pub struct Authorization {
    url: Url,
    state: String,
    pkce_verifier: PkceCodeVerifier,
}

impl Authorization {
    /// Returns the URL where the resource owner should authorize the app.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Returns the CSRF state expected in the redirect.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }
}

impl fmt::Debug for Authorization {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Authorization")
            .field("url", &self.url)
            .field("state", &self.state)
            .field("pkce_verifier", &"[REDACTED]")
            .finish()
    }
}

/// Tokens issued by Roblox.
#[derive(Clone)]
pub struct Token {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    token_type: String,
    expires_in: Option<u64>,
    scopes: Vec<String>,
}

impl Token {
    /// Returns the bearer access token.
    #[must_use]
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// Returns the single-use refresh token, when one was issued.
    #[must_use]
    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_deref()
    }

    /// Returns the OpenID Connect ID token, when one was issued.
    #[must_use]
    pub fn id_token(&self) -> Option<&str> {
        self.id_token.as_deref()
    }

    /// Returns the token type reported by Roblox.
    #[must_use]
    pub fn token_type(&self) -> &str {
        &self.token_type
    }

    /// Returns the access-token lifetime in seconds.
    #[must_use]
    pub const fn expires_in(&self) -> Option<u64> {
        self.expires_in
    }

    /// Returns the granted OAuth scopes.
    #[must_use]
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Token")
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("id_token", &self.id_token.as_ref().map(|_| "[REDACTED]"))
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("scopes", &self.scopes)
            .finish()
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    token_type: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: String,
}

impl From<TokenResponse> for Token {
    fn from(response: TokenResponse) -> Self {
        Self {
            access_token: response.access_token,
            refresh_token: response.refresh_token,
            id_token: response.id_token,
            token_type: response.token_type,
            expires_in: response.expires_in,
            scopes: split_scopes(&response.scope),
        }
    }
}

/// The owner that granted access to OAuth resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceOwner {
    /// The owner identifier.
    pub id: String,
    /// The owner type, such as `User` or `Group`.
    #[serde(rename = "type")]
    pub kind: String,
}

/// Identifiers authorized for one resource kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceIds {
    /// Authorized identifiers.
    pub ids: Vec<String>,
}

/// OAuth resources granted by one owner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    /// The resource owner.
    pub owner: ResourceOwner,
    /// Resource kinds mapped to their authorized identifiers.
    #[serde(default)]
    pub resources: BTreeMap<String, ResourceIds>,
}

/// Resources authorized for an access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResources {
    /// Grants grouped by resource owner.
    #[serde(default)]
    pub resource_infos: Vec<ResourceInfo>,
}

impl TokenResources {
    /// Returns sorted, unique universe identifiers from all grants.
    #[must_use]
    pub fn universe_ids(&self) -> Vec<u64> {
        let mut ids = self
            .resource_infos
            .iter()
            .filter_map(|info| info.resources.get("universe"))
            .flat_map(|resource| resource.ids.iter())
            .filter_map(|id| id.parse::<u64>().ok())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

/// Information returned by OAuth token introspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Whether the token is active according to its lifetime.
    pub active: bool,
    /// The token identifier.
    #[serde(default)]
    pub jti: Option<String>,
    /// The token issuer.
    #[serde(default)]
    pub iss: Option<String>,
    /// The token type.
    #[serde(default)]
    pub token_type: Option<String>,
    /// The OAuth client identifier.
    #[serde(default)]
    pub client_id: Option<String>,
    /// The token audience.
    #[serde(default)]
    pub aud: Option<String>,
    /// The subject identifier.
    #[serde(default)]
    pub sub: Option<String>,
    /// The space-delimited scopes as returned by Roblox.
    #[serde(default)]
    pub scope: Option<String>,
    /// The expiration timestamp.
    #[serde(default)]
    pub exp: Option<u64>,
    /// The issuance timestamp.
    #[serde(default)]
    pub iat: Option<u64>,
}

impl TokenInfo {
    /// Returns the token's scopes.
    #[must_use]
    pub fn scopes(&self) -> Vec<String> {
        self.scope.as_deref().map_or_else(Vec::new, split_scopes)
    }
}

/// OpenID Connect user information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// The stable Roblox user identifier.
    pub sub: String,
    /// The display name, when the `profile` scope was granted.
    #[serde(default)]
    pub name: Option<String>,
    /// The display name alias.
    #[serde(default)]
    pub nickname: Option<String>,
    /// The Roblox username.
    #[serde(default)]
    pub preferred_username: Option<String>,
    /// The account creation timestamp.
    #[serde(default)]
    pub created_at: Option<u64>,
    /// The Roblox profile URL.
    #[serde(default)]
    pub profile: Option<String>,
    /// The avatar-headshot URL.
    #[serde(default)]
    pub picture: Option<String>,
}

/// Roblox's OpenID Connect discovery document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discovery {
    /// The issuer identifier.
    pub issuer: String,
    /// The authorization endpoint.
    pub authorization_endpoint: String,
    /// The token endpoint.
    pub token_endpoint: String,
    /// The introspection endpoint.
    pub introspection_endpoint: String,
    /// The revocation endpoint.
    pub revocation_endpoint: String,
    /// The token-resources endpoint.
    pub resources_endpoint: String,
    /// The user-information endpoint.
    pub userinfo_endpoint: String,
    /// The JSON Web Key Set endpoint.
    pub jwks_uri: String,
    /// Supported OAuth and OpenID Connect scopes.
    #[serde(default)]
    pub scopes_supported: Vec<String>,
    /// Supported OpenID Connect claims.
    #[serde(default)]
    pub claims_supported: Vec<String>,
}

/// A Roblox OAuth client.
pub struct Client {
    inner: InnerClient,
    http: reqwest::Client,
    client_id: String,
    client_secret: Option<String>,
    redirect_url: String,
    token_url: String,
    introspection_url: String,
    resources_url: String,
    revoke_url: String,
    uses_relay: bool,
}

impl Client {
    /// Creates an OAuth client.
    ///
    /// Pass `None` for a public client that cannot hold a client secret.
    ///
    /// # Errors
    ///
    /// Returns an error when an OAuth endpoint or redirect URL is invalid.
    pub fn new(
        client_id: impl Into<String>,
        client_secret: Option<String>,
        redirect_url: impl Into<String>,
    ) -> Result<Self> {
        let client_id = client_id.into();
        let redirect_url = redirect_url.into();

        if client_id.is_empty() {
            return Err(Error::InvalidCredential {
                kind: "OAuth client ID",
            });
        }

        let mut inner = BasicClient::new(ClientId::new(client_id.clone()))
            .set_auth_uri(
                AuthUrl::new(String::from(AUTH_URL))
                    .map_err(|error| Error::OAuth(error.to_string()))?,
            )
            .set_token_uri(
                TokenUrl::new(String::from(TOKEN_URL))
                    .map_err(|error| Error::OAuth(error.to_string()))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(redirect_url.clone())
                    .map_err(|error| Error::OAuth(error.to_string()))?,
            )
            .set_auth_type(AuthType::RequestBody);

        if let Some(secret) = &client_secret {
            inner = inner.set_client_secret(ClientSecret::new(secret.clone()));
        }

        Ok(Self {
            inner,
            http: reqwest::Client::new(),
            client_id,
            client_secret,
            redirect_url,
            token_url: String::from(TOKEN_URL),
            introspection_url: String::from(INTROSPECTION_URL),
            resources_url: String::from(RESOURCES_URL),
            revoke_url: String::from(REVOKE_URL),
            uses_relay: false,
        })
    }

    /// Routes secret-bearing OAuth token operations through a relay.
    ///
    /// The authorization endpoint remains Roblox-hosted.
    ///
    /// # Errors
    ///
    /// Returns an error when the relay token URL is invalid.
    pub fn with_relay(mut self, relay_url: &str) -> Result<Self> {
        let relay_url = relay_url.trim_end_matches('/');
        self.token_url = format!("{relay_url}/oauth/v1/token");
        self.introspection_url = format!("{relay_url}/oauth/v1/token/introspect");
        self.resources_url = format!("{relay_url}/oauth/v1/token/resources");
        self.revoke_url = format!("{relay_url}/oauth/v1/token/revoke");
        self.inner = self.inner.set_token_uri(
            TokenUrl::new(self.token_url.clone())
                .map_err(|error| Error::OAuth(error.to_string()))?,
        );
        self.uses_relay = true;
        Ok(self)
    }

    /// Starts an OAuth authorization request with PKCE.
    #[must_use]
    pub fn authorize(&self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Authorization {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let mut request = self
            .inner
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge);

        for scope in scopes {
            request = request.add_scope(Scope::new(scope.into()));
        }

        let (url, state) = request.url();
        Authorization {
            url,
            state: state.secret().clone(),
            pkce_verifier,
        }
    }

    /// Exchanges an authorization code for tokens after validating CSRF state.
    ///
    /// # Errors
    ///
    /// Returns an error for a state mismatch, failed request, rejected token
    /// exchange, or invalid response.
    pub async fn exchange(
        &self,
        code: &str,
        returned_state: &str,
        authorization: Authorization,
    ) -> Result<Token> {
        if returned_state != authorization.state {
            return Err(Error::OAuthStateMismatch);
        }

        let form = ExchangeForm {
            grant_type: "authorization_code",
            code,
            code_verifier: authorization.pkce_verifier.secret(),
            client_id: &self.client_id,
            client_secret: self.form_client_secret(),
            redirect_uri: &self.redirect_url,
        };
        self.request_token(&form).await
    }

    /// Uses a refresh token to obtain a new token set.
    ///
    /// Roblox refresh tokens are single-use. Persist the new refresh token
    /// before discarding the old one.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails, Roblox rejects it, or the
    /// response is invalid.
    pub async fn refresh(&self, refresh_token: &str) -> Result<Token> {
        let form = RefreshForm {
            grant_type: "refresh_token",
            refresh_token,
            client_id: &self.client_id,
            client_secret: self.form_client_secret(),
        };
        self.request_token(&form).await
    }

    /// Introspects an access, refresh, or ID token.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails, Roblox rejects it, or the
    /// response is invalid.
    pub async fn introspect(&self, token: &str) -> Result<TokenInfo> {
        let form = ClientTokenForm {
            token,
            client_id: &self.client_id,
            client_secret: self.form_client_secret(),
        };
        let response = self
            .http
            .post(&self.introspection_url)
            .form(&form)
            .send()
            .await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }

    /// Returns resources authorized for an access token.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails, Roblox rejects it, or the
    /// response is invalid.
    pub async fn resources(&self, access_token: &str) -> Result<TokenResources> {
        let form = ClientTokenForm {
            token: access_token,
            client_id: &self.client_id,
            client_secret: self.form_client_secret(),
        };
        let response = self
            .http
            .post(&self.resources_url)
            .form(&form)
            .send()
            .await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }

    /// Revokes an authorization session through its refresh token.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn revoke(&self, refresh_token: &str) -> Result<()> {
        let form = ClientTokenForm {
            token: refresh_token,
            client_id: &self.client_id,
            client_secret: self.form_client_secret(),
        };
        let response = self.http.post(&self.revoke_url).form(&form).send().await?;
        let _response = check_status(response).await?;
        Ok(())
    }

    /// Returns OpenID Connect user information.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails, Roblox rejects it, or the
    /// response is invalid.
    pub async fn user_info(&self, access_token: &str) -> Result<UserInfo> {
        let response = self
            .http
            .get(USER_INFO_URL)
            .bearer_auth(access_token)
            .send()
            .await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }

    /// Downloads Roblox's OpenID Connect discovery document.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails, Roblox rejects it, or the
    /// response is invalid.
    pub async fn discovery(&self) -> Result<Discovery> {
        let response = self.http.get(DISCOVERY_URL).send().await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }

    async fn request_token(&self, form: &(impl Serialize + Sync)) -> Result<Token> {
        let response = self.http.post(&self.token_url).form(form).send().await?;
        let response = check_status(response).await?;
        let response: TokenResponse = response.json().await?;
        Ok(response.into())
    }

    fn form_client_secret(&self) -> Option<&str> {
        self.client_secret
            .as_deref()
            .or_else(|| self.uses_relay.then_some(""))
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Client")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("redirect_url", &self.redirect_url)
            .field("token_url", &self.token_url)
            .field("introspection_url", &self.introspection_url)
            .field("resources_url", &self.resources_url)
            .field("revoke_url", &self.revoke_url)
            .field("uses_relay", &self.uses_relay)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize)]
struct ExchangeForm<'form> {
    grant_type: &'static str,
    code: &'form str,
    code_verifier: &'form str,
    client_id: &'form str,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_secret: Option<&'form str>,
    redirect_uri: &'form str,
}

#[derive(Debug, Serialize)]
struct RefreshForm<'form> {
    grant_type: &'static str,
    refresh_token: &'form str,
    client_id: &'form str,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_secret: Option<&'form str>,
}

#[derive(Debug, Serialize)]
struct ClientTokenForm<'form> {
    token: &'form str,
    client_id: &'form str,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_secret: Option<&'form str>,
}

fn split_scopes(scopes: &str) -> Vec<String> {
    scopes.split_whitespace().map(String::from).collect()
}
