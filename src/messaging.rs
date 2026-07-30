//! Cross-server messaging operations.

use reqwest::Method;
use serde::Serialize;

use crate::client::universe_path;
use crate::{Client, Result};

#[derive(Debug, Serialize)]
struct PublishRequest<'message> {
    topic: &'message str,
    message: &'message str,
}

include!("generated/messaging.rs");

/// Messaging operations for an authenticated client.
#[derive(Debug, Clone, Copy)]
pub struct Messaging<'client> {
    client: &'client Client,
}

impl Client {
    /// Returns messaging operations.
    #[must_use]
    pub const fn messaging(&self) -> Messaging<'_> {
        Messaging { client: self }
    }
}

impl Messaging<'_> {
    /// Publishes a message to live servers in a universe.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn publish(&self, universe_id: u64, topic: &str, message: &str) -> Result<()> {
        let path = universe_path(universe_id, ":publishMessage");
        let builder = self
            .client
            .request(Method::POST, &path)?
            .json(&PublishRequest { topic, message });
        let _response = self.client.send(builder).await?;
        Ok(())
    }
}
