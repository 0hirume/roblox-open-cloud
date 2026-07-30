use std::fmt;

use bytes::Bytes;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC};
use reqwest::header::HeaderMap;
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::auth::Credentials;
use crate::error::{Result, check_status};
use crate::operation::{Endpoint, HttpMethod, OperationRequest};

const DEFAULT_BASE_URL: &str = "https://apis.roblox.com/";

const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Common options accepted by list operations.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOptions {
    /// A service-specific filter expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// A service-specific ordering expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_by: Option<String>,
    /// A continuation token returned by a previous request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
    /// The requested maximum number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_page_size: Option<u32>,
    /// Whether deleted resources should be included.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub show_deleted: bool,
}

/// A successful response from an operation without a dedicated response model.
#[derive(Debug, Clone)]
pub struct RawResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl RawResponse {
    /// Returns the response status.
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns the response headers.
    #[must_use]
    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Returns the response body.
    #[must_use]
    pub const fn body(&self) -> &Bytes {
        &self.body
    }

    /// Consumes the response and returns its body.
    #[must_use]
    pub fn into_body(self) -> Bytes {
        self.body
    }

    /// Deserializes the response body as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error when the body is not valid JSON for `T`.
    pub fn json<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        Ok(serde_json::from_slice(&self.body)?)
    }
}

/// An authenticated Roblox Open Cloud client.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: Url,
    credentials: Credentials,
}

impl Client {
    /// Creates a client from explicit credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the built-in Roblox API URL cannot be parsed.
    pub fn new(credentials: Credentials) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::new(),
            base_url: Url::parse(DEFAULT_BASE_URL)?,
            credentials,
        })
    }

    /// Creates a client authenticated by an API key.
    ///
    /// # Errors
    ///
    /// Returns an error when the key is empty or the default URL is invalid.
    pub fn api_key(key: impl Into<String>) -> Result<Self> {
        Self::new(Credentials::api_key(key)?)
    }

    /// Creates a client authenticated by an OAuth access token.
    ///
    /// # Errors
    ///
    /// Returns an error when the token is empty or the default URL is invalid.
    pub fn oauth(token: impl Into<String>) -> Result<Self> {
        Self::new(Credentials::oauth(token)?)
    }

    /// Replaces the Roblox API base URL.
    ///
    /// This is primarily useful for proxies and deterministic tests.
    ///
    /// # Errors
    ///
    /// Returns an error when `base_url` is not a valid absolute URL.
    pub fn with_base_url(mut self, base_url: impl AsRef<str>) -> Result<Self> {
        let mut base_url = Url::parse(base_url.as_ref())?;
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }
        self.base_url = base_url;
        Ok(self)
    }

    /// Returns the configured authentication mode.
    #[must_use]
    pub const fn authentication(&self) -> crate::Authentication {
        self.credentials.authentication()
    }

    pub(crate) const fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// Starts a request to a supported endpoint.
    #[must_use]
    pub fn operation(&self, endpoint: Endpoint) -> OperationRequest<'_> {
        OperationRequest::new(self, endpoint)
    }

    pub(crate) fn request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        self.request_with_authentication(method, path, true)
    }

    pub(crate) fn endpoint_request(
        &self,
        endpoint: Endpoint,
        path: &str,
    ) -> Result<RequestBuilder> {
        let support = endpoint.authentication();
        let authenticate = match self.authentication() {
            crate::Authentication::ApiKey if support.api_key() => true,
            crate::Authentication::OAuth if support.oauth() => true,
            _ if support.unauthenticated() => false,
            authentication @ (crate::Authentication::ApiKey | crate::Authentication::OAuth) => {
                return Err(crate::Error::UnsupportedAuthentication {
                    operation: endpoint.summary(),
                    authentication,
                });
            }
        };

        self.request_with_authentication(endpoint.method().into(), path, authenticate)
    }

    pub(crate) fn request_with_authentication(
        &self,
        method: Method,
        path: &str,
        authenticate: bool,
    ) -> Result<RequestBuilder> {
        let url = self.base_url.join(path.trim_start_matches('/'))?;
        let builder = self.http.request(method, url);
        Ok(if authenticate {
            self.credentials.authorize(builder)
        } else {
            builder
        })
    }

    pub(crate) async fn send_json<T>(&self, builder: RequestBuilder) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = check_status(builder.send().await?).await?;
        Ok(response.json().await?)
    }

    pub(crate) async fn send(&self, builder: RequestBuilder) -> Result<reqwest::Response> {
        check_status(builder.send().await?).await
    }

    pub(crate) async fn send_raw(&self, builder: RequestBuilder) -> Result<RawResponse> {
        let response = check_status(builder.send().await?).await?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.bytes().await?;
        Ok(RawResponse {
            status,
            headers,
            body,
        })
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("credentials", &self.credentials)
            .finish_non_exhaustive()
    }
}

pub(crate) fn encode_path_segment(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, PATH_SEGMENT).to_string()
}

pub(crate) fn item_path(collection_path: &str, item_id: &str) -> String {
    format!("{collection_path}/{}", encode_path_segment(item_id))
}

pub(crate) fn universe_path(universe_id: u64, suffix: &str) -> String {
    format!("/cloud/v2/universes/{universe_id}{suffix}")
}

pub(crate) fn extract_revision(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    object
        .get("revisionId")
        .or_else(|| object.get("etag"))
        .and_then(serde_json::Value::as_str)
        .map(String::from)
}

impl From<HttpMethod> for Method {
    fn from(method: HttpMethod) -> Self {
        match method {
            HttpMethod::Delete => Self::DELETE,
            HttpMethod::Get => Self::GET,
            HttpMethod::Head => Self::HEAD,
            HttpMethod::Options => Self::OPTIONS,
            HttpMethod::Patch => Self::PATCH,
            HttpMethod::Post => Self::POST,
            HttpMethod::Put => Self::PUT,
            HttpMethod::Trace => Self::TRACE,
        }
    }
}
