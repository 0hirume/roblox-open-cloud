use std::fmt;
use std::fmt::Display;

use bytes::Bytes;
use reqwest::multipart::{Form, Part};
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

/// A file supplied to a multipart Open Cloud operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    contents: Bytes,
    file_name: Option<String>,
    content_type: Option<String>,
}

impl File {
    /// Creates a file from its contents.
    #[must_use]
    pub fn new(contents: impl Into<Bytes>) -> Self {
        Self {
            contents: contents.into(),
            file_name: None,
            content_type: None,
        }
    }

    /// Sets the file name sent in the multipart disposition.
    #[must_use]
    pub fn with_name(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    /// Sets the file's MIME content type.
    #[must_use]
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    /// Converts this value into a multipart part.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured content type is invalid.
    pub fn into_part(self) -> Result<Part> {
        let mut part = Part::stream(self.contents);
        if let Some(file_name) = self.file_name {
            part = part.file_name(file_name);
        }
        if let Some(content_type) = self.content_type {
            part = part.mime_str(&content_type)?;
        }
        Ok(part)
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

    /// Substitutes one typed, percent-encoded path parameter.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is not scalar or the endpoint has no
    /// parameter with `name`.
    pub fn path_serialized(self, name: &str, value: &impl Serialize) -> Result<Self> {
        let mut values = parameter_values(value)?.into_iter();
        let first = values.next().ok_or_else(|| {
            Error::InvalidResponse(format!("path parameter `{name}` serialized to no values"))
        })?;
        if values.next().is_some() {
            return Err(Error::InvalidResponse(format!(
                "path parameter `{name}` serialized to multiple values"
            )));
        }
        self.path(name, first)
    }

    /// Substitutes an optional typed path parameter when present.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is not scalar or the endpoint has no
    /// parameter with `name`.
    pub fn path_optional_serialized<T>(self, name: &str, value: Option<&T>) -> Result<Self>
    where
        T: Serialize,
    {
        match value {
            Some(value) => self.path_serialized(name, value),
            None => Ok(self),
        }
    }

    /// Adds one query parameter.
    #[must_use]
    pub fn query(mut self, name: impl Into<String>, value: impl Display) -> Self {
        self.query.push((name.into(), value.to_string()));
        self
    }

    /// Adds a typed query parameter using OpenAPI form serialization.
    ///
    /// # Errors
    ///
    /// Returns an error when the value cannot be serialized.
    pub fn query_serialized(
        mut self,
        name: impl Into<String>,
        value: &impl Serialize,
        explode: bool,
    ) -> Result<Self> {
        let name = name.into();
        let values = parameter_values(value)?;
        if explode {
            self.query.extend(
                values
                    .into_iter()
                    .map(|serialized| (name.clone(), serialized)),
            );
        } else if !values.is_empty() {
            self.query.push((name, values.join(",")));
        }
        Ok(self)
    }

    /// Adds an optional typed query parameter when present.
    ///
    /// # Errors
    ///
    /// Returns an error when the value cannot be serialized.
    pub fn query_optional_serialized<T>(
        self,
        name: impl Into<String>,
        value: Option<&T>,
        explode: bool,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        match value {
            Some(value) => self.query_serialized(name, value, explode),
            None => Ok(self),
        }
    }

    /// Adds one request header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Adds a typed request header.
    ///
    /// # Errors
    ///
    /// Returns an error when the value cannot be serialized.
    pub fn header_serialized(
        mut self,
        name: impl Into<String>,
        value: &impl Serialize,
        explode: bool,
    ) -> Result<Self> {
        let name = name.into();
        let values = parameter_values(value)?;
        if explode {
            self.headers.extend(
                values
                    .into_iter()
                    .map(|serialized| (name.clone(), serialized)),
            );
        } else if !values.is_empty() {
            self.headers.push((name, values.join(",")));
        }
        Ok(self)
    }

    /// Adds an optional typed request header when present.
    ///
    /// # Errors
    ///
    /// Returns an error when the value cannot be serialized.
    pub fn header_optional_serialized<T>(
        self,
        name: impl Into<String>,
        value: Option<&T>,
        explode: bool,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        match value {
            Some(value) => self.header_serialized(name, value, explode),
            None => Ok(self),
        }
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

fn parameter_values(input: &impl Serialize) -> Result<Vec<String>> {
    let serialized = serde_json::to_value(input)?;
    match serialized {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(values) => values
            .into_iter()
            .map(|item| parameter_text(&item))
            .collect(),
        value @ (serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_)
        | serde_json::Value::Object(_)) => Ok(vec![parameter_text(&value)?]),
    }
}

fn parameter_text(value: &serde_json::Value) -> Result<String> {
    match value {
        serde_json::Value::Null => Ok(String::new()),
        serde_json::Value::Bool(value) => Ok(value.to_string()),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        serde_json::Value::String(value) => Ok(value.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Ok(serde_json::to_string(value)?)
        }
    }
}

pub(crate) fn multipart_text(value: &impl Serialize) -> Result<String> {
    parameter_text(&serde_json::to_value(value)?)
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
