//! Ordered DataStore operations.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::{encode_path_segment, item_path, universe_path};
use crate::{Client, ListOptions, Result};

/// An ordered DataStore entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The resource path.
    #[serde(default)]
    pub path: String,
    /// The numeric entry value.
    pub value: f64,
    /// The entry identifier.
    #[serde(default)]
    pub id: String,
}

include!("generated/ordered_datastore.rs");

/// A page of ordered DataStore entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPage {
    /// The returned entries.
    #[serde(default)]
    pub ordered_data_store_entries: Vec<Entry>,
    /// A continuation token for the next page.
    #[serde(default)]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct ValueRequest {
    value: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct IncrementRequest {
    amount: f64,
}

/// Ordered DataStore operations for an authenticated client.
#[derive(Debug, Clone, Copy)]
pub struct OrderedDataStores<'client> {
    client: &'client Client,
}

impl Client {
    /// Returns ordered DataStore operations.
    #[must_use]
    pub const fn ordered_data_stores(&self) -> OrderedDataStores<'_> {
        OrderedDataStores { client: self }
    }
}

impl OrderedDataStores<'_> {
    /// Lists entries.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn list(
        &self,
        universe_id: u64,
        data_store_id: &str,
        scope: &str,
        options: &ListOptions,
    ) -> Result<EntryPage> {
        let path = entries_path(universe_id, data_store_id, scope);
        let builder = self.client.request(Method::GET, &path)?.query(options);
        self.client.send_json(builder).await
    }

    /// Creates an entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn create(
        &self,
        universe_id: u64,
        data_store_id: &str,
        scope: &str,
        entry_id: &str,
        value: f64,
    ) -> Result<Entry> {
        let path = entries_path(universe_id, data_store_id, scope);
        let builder = self
            .client
            .request(Method::POST, &path)?
            .query(&[("id", entry_id)])
            .json(&ValueRequest { value });
        self.client.send_json(builder).await
    }

    /// Returns an entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn get(
        &self,
        universe_id: u64,
        data_store_id: &str,
        scope: &str,
        entry_id: &str,
    ) -> Result<Entry> {
        let path = item_path(&entries_path(universe_id, data_store_id, scope), entry_id);
        let builder = self.client.request(Method::GET, &path)?;
        self.client.send_json(builder).await
    }

    /// Replaces an entry's value.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn update(
        &self,
        universe_id: u64,
        data_store_id: &str,
        scope: &str,
        entry_id: &str,
        value: f64,
    ) -> Result<Entry> {
        let path = item_path(&entries_path(universe_id, data_store_id, scope), entry_id);
        let builder = self
            .client
            .request(Method::PATCH, &path)?
            .json(&ValueRequest { value });
        self.client.send_json(builder).await
    }

    /// Deletes an entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn delete(
        &self,
        universe_id: u64,
        data_store_id: &str,
        scope: &str,
        entry_id: &str,
    ) -> Result<()> {
        let path = item_path(&entries_path(universe_id, data_store_id, scope), entry_id);
        let builder = self.client.request(Method::DELETE, &path)?;
        let _response = self.client.send(builder).await?;
        Ok(())
    }

    /// Increments an entry's value.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn increment(
        &self,
        universe_id: u64,
        data_store_id: &str,
        scope: &str,
        entry_id: &str,
        amount: f64,
    ) -> Result<Entry> {
        let path = format!(
            "{}:increment",
            item_path(&entries_path(universe_id, data_store_id, scope), entry_id)
        );
        let builder = self
            .client
            .request(Method::POST, &path)?
            .json(&IncrementRequest { amount });
        self.client.send_json(builder).await
    }
}

fn entries_path(universe_id: u64, data_store_id: &str, scope: &str) -> String {
    let data_store_path = item_path(
        &universe_path(universe_id, "/ordered-data-stores"),
        data_store_id,
    );
    format!(
        "{data_store_path}/scopes/{}/entries",
        encode_path_segment(scope)
    )
}
