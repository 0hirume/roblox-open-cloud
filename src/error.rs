use reqwest::{Response, StatusCode};
use thiserror::Error;

/// An error returned by the Roblox Open Cloud client.
#[derive(Debug, Error)]
pub enum Error {
    /// An HTTP request could not be built, sent, or decoded.
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// Roblox returned a non-success status.
    #[error("API returned error status {status}: {body}")]
    Api {
        /// The returned HTTP status.
        status: StatusCode,
        /// The response body, when one was returned.
        body: String,
    },

    /// A JSON value could not be encoded or decoded.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// A configured URL is invalid.
    #[error("URL error: {0}")]
    Url(#[from] url::ParseError),

    /// OAuth authorization or token handling failed.
    #[error("OAuth error: {0}")]
    OAuth(String),

    /// OAuth returned a different state than the authorization request.
    #[error("OAuth state mismatch")]
    OAuthStateMismatch,

    /// A credential was empty or otherwise invalid.
    #[error("invalid {kind}")]
    InvalidCredential {
        /// The rejected credential kind.
        kind: &'static str,
    },

    /// The selected operation does not support the configured authentication.
    #[error("{operation} does not support {authentication}")]
    UnsupportedAuthentication {
        /// The operation that rejected the authentication mode.
        operation: &'static str,
        /// The rejected authentication mode.
        authentication: crate::Authentication,
    },

    /// Roblox returned a response that did not match the documented shape.
    #[error("invalid API response: {0}")]
    InvalidResponse(String),

    /// A generated operation request is missing path substitutions.
    #[error("missing path parameters in {path}")]
    MissingPathParameters {
        /// The unresolved operation path.
        path: String,
    },

    /// A path substitution does not exist on the selected endpoint.
    #[error("unknown path parameter {parameter} for {path}")]
    UnknownPathParameter {
        /// The unknown parameter name.
        parameter: String,
        /// The endpoint path template.
        path: &'static str,
    },
}

/// A result returned by this crate.
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) async fn check_status(response: Response) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(Error::Api { status, body })
}
