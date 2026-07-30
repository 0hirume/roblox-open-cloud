use std::fmt;
use std::fmt::Display;

use bytes::Bytes;
use reqwest::multipart::Form;
use serde::Serialize;

use crate::client::encode_path_segment;
use crate::{Client, Error, RawResponse, Result};

/// An HTTP method used by a supported endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// `DELETE`
    Delete,
    /// `GET`
    Get,
    /// `HEAD`
    Head,
    /// `OPTIONS`
    Options,
    /// `PATCH`
    Patch,
    /// `POST`
    Post,
    /// `PUT`
    Put,
    /// `TRACE`
    Trace,
}

/// The documented stability of a supported endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stability {
    /// Roblox marks the endpoint as stable.
    Stable,
    /// Roblox marks the endpoint as beta.
    Beta,
    /// The endpoint supports modern authentication and inherits the documented
    /// beta guarantee for older Open Cloud endpoints.
    LegacyBeta,
}

/// Authentication modes accepted by an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthenticationSupport {
    api_key: bool,
    oauth: bool,
    unauthenticated: bool,
}

impl AuthenticationSupport {
    pub(crate) const fn new(api_key: bool, oauth: bool, unauthenticated: bool) -> Self {
        Self {
            api_key,
            oauth,
            unauthenticated,
        }
    }

    /// Returns whether API-key authentication is accepted.
    #[must_use]
    pub const fn api_key(self) -> bool {
        self.api_key
    }

    /// Returns whether OAuth authentication is accepted.
    #[must_use]
    pub const fn oauth(self) -> bool {
        self.oauth
    }

    /// Returns whether the endpoint can be called without authentication.
    #[must_use]
    pub const fn unauthenticated(self) -> bool {
        self.unauthenticated
    }
}

/// Metadata for one supported Roblox endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoint {
    method: HttpMethod,
    path: &'static str,
    summary: &'static str,
    stability: Stability,
    authentication: AuthenticationSupport,
    scopes: &'static [&'static str],
}

impl Endpoint {
    pub(crate) const fn new(
        method: HttpMethod,
        path: &'static str,
        summary: &'static str,
        stability: Stability,
        authentication: AuthenticationSupport,
        scopes: &'static [&'static str],
    ) -> Self {
        Self {
            method,
            path,
            summary,
            stability,
            authentication,
            scopes,
        }
    }

    /// Returns the HTTP method.
    #[must_use]
    pub const fn method(self) -> HttpMethod {
        self.method
    }

    /// Returns the path template.
    #[must_use]
    pub const fn path(self) -> &'static str {
        self.path
    }

    /// Returns the Creator Docs operation summary.
    #[must_use]
    pub const fn summary(self) -> &'static str {
        self.summary
    }

    /// Returns the endpoint stability.
    #[must_use]
    pub const fn stability(self) -> Stability {
        self.stability
    }

    /// Returns supported authentication modes.
    #[must_use]
    pub const fn authentication(self) -> AuthenticationSupport {
        self.authentication
    }

    /// Returns the documented permission scopes.
    #[must_use]
    pub const fn scopes(self) -> &'static [&'static str] {
        self.scopes
    }
}

enum RequestBody {
    Bytes(Bytes),
    Form(Vec<(String, String)>),
    Json(serde_json::Value),
    Multipart(Form),
    Text(String),
}

impl fmt::Debug for RequestBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes(bytes) => formatter.debug_tuple("Bytes").field(&bytes.len()).finish(),
            Self::Form(fields) => formatter.debug_tuple("Form").field(&fields.len()).finish(),
            Self::Json(_) => formatter.write_str("Json([REDACTED])"),
            Self::Multipart(_) => formatter.write_str("Multipart([REDACTED])"),
            Self::Text(text) => formatter.debug_tuple("Text").field(&text.len()).finish(),
        }
    }
}

/// A request being prepared for a supported endpoint.
pub struct OperationRequest<'client> {
    client: &'client Client,
    endpoint: Endpoint,
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Option<RequestBody>,
}

impl<'client> OperationRequest<'client> {
    pub(crate) fn new(client: &'client Client, endpoint: Endpoint) -> Self {
        Self {
            client,
            endpoint,
            path: String::from(endpoint.path()),
            query: Vec::new(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Substitutes one percent-encoded path parameter.
    ///
    /// # Errors
    ///
    /// Returns an error when the endpoint has no parameter with `name`.
    pub fn path(mut self, name: &str, value: impl Display) -> Result<Self> {
        let placeholder = format!("{{{name}}}");
        if !self.path.contains(&placeholder) {
            return Err(Error::UnknownPathParameter {
                parameter: String::from(name),
                path: self.endpoint.path(),
            });
        }
        self.path = self
            .path
            .replace(&placeholder, &encode_path_segment(&value.to_string()));
        Ok(self)
    }

    /// Adds one query parameter.
    #[must_use]
    pub fn query(mut self, name: impl Into<String>, value: impl Display) -> Self {
        self.query.push((name.into(), value.to_string()));
        self
    }

    /// Adds one request header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Sets a JSON request body.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` cannot be serialized as JSON.
    pub fn json(mut self, value: &impl Serialize) -> Result<Self> {
        self.body = Some(RequestBody::Json(serde_json::to_value(value)?));
        Ok(self)
    }

    /// Sets a byte request body.
    #[must_use]
    pub fn bytes(mut self, value: impl Into<Bytes>) -> Self {
        self.body = Some(RequestBody::Bytes(value.into()));
        self
    }

    /// Sets a UTF-8 text request body.
    #[must_use]
    pub fn text(mut self, value: impl Into<String>) -> Self {
        self.body = Some(RequestBody::Text(value.into()));
        self
    }

    /// Sets a URL-encoded form request body.
    #[must_use]
    pub fn form(mut self, fields: Vec<(String, String)>) -> Self {
        self.body = Some(RequestBody::Form(fields));
        self
    }

    /// Sets a multipart request body.
    #[must_use]
    pub fn multipart(mut self, form: Form) -> Self {
        self.body = Some(RequestBody::Multipart(form));
        self
    }

    /// Sends the request.
    ///
    /// # Errors
    ///
    /// Returns an error when path parameters remain unresolved, the configured
    /// authentication is unsupported, the request fails, or Roblox rejects it.
    pub async fn send(self) -> Result<RawResponse> {
        if self.path.contains('{') {
            return Err(Error::MissingPathParameters { path: self.path });
        }

        let mut builder = self.client.endpoint_request(self.endpoint, &self.path)?;
        if !self.query.is_empty() {
            builder = builder.query(&self.query);
        }
        for (name, value) in self.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = self.body {
            builder = match body {
                RequestBody::Bytes(value) => builder.body(value),
                RequestBody::Form(value) => builder.form(&value),
                RequestBody::Json(value) => builder.json(&value),
                RequestBody::Multipart(value) => builder.multipart(value),
                RequestBody::Text(value) => builder.body(value),
            };
        }

        self.client.send_raw(builder).await
    }
}

impl fmt::Debug for OperationRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OperationRequest")
            .field("endpoint", &self.endpoint)
            .field("path", &self.path)
            .field("query", &self.query)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .finish_non_exhaustive()
    }
}
