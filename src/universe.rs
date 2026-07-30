//! Universe operations.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::universe_path;
use crate::{Client, Result};

/// A Roblox universe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Universe {
    /// The universe's display name.
    pub display_name: String,
}

include!("generated/universe.rs");

/// Universe operations for an authenticated client.
#[derive(Debug, Clone, Copy)]
pub struct Universes<'client> {
    client: &'client Client,
}

impl Client {
    /// Returns universe operations.
    #[must_use]
    pub const fn universes(&self) -> Universes<'_> {
        Universes { client: self }
    }
}

impl Universes<'_> {
    /// Returns a universe.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn get(&self, universe_id: u64) -> Result<Universe> {
        let path = universe_path(universe_id, "");
        let builder = self.client.request(Method::GET, &path)?;
        self.client.send_json(builder).await
    }
}
