//! Standard DataStore operations.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{encode_path_segment, extract_revision, item_path, universe_path};
use crate::{Client, ListOptions, Result};

/// A standard DataStore.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataStore {
    /// The resource path.
    pub path: String,
    /// The DataStore identifier.
    pub id: String,
    /// The creation timestamp.
    pub create_time: String,
    /// The expiration timestamp, when the DataStore is deleted.
    #[serde(default)]
    pub expire_time: Option<String>,
    /// The DataStore state.
    #[serde(default)]
    pub state: Option<String>,
}

/// A page of standard DataStores.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataStorePage {
    /// The returned DataStores.
    #[serde(default)]
    pub data_stores: Vec<DataStore>,
    /// A continuation token for the next page.
    #[serde(default)]
    pub next_page_token: Option<String>,
}

/// Metadata for a standard DataStore entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The resource path.
    pub path: String,
    /// The entry identifier.
    pub id: String,
    /// The creation timestamp.
    #[serde(default)]
    pub create_time: Option<String>,
    /// The current revision identifier.
    #[serde(default)]
    pub revision_id: Option<String>,
    /// The current revision timestamp.
    #[serde(default)]
    pub revision_create_time: Option<String>,
    /// The entry state.
    #[serde(default)]
    pub state: Option<String>,
    /// The entity tag used for conditional updates.
    #[serde(default)]
    pub etag: Option<String>,
}

/// A page of standard DataStore entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPage {
    /// The returned entries.
    #[serde(default)]
    pub data_store_entries: Vec<Entry>,
    /// A continuation token for the next page.
    #[serde(default)]
    pub next_page_token: Option<String>,
}

/// Standard DataStore operations for an authenticated client.
#[derive(Debug, Clone, Copy)]
pub struct DataStores<'client> {
    client: &'client Client,
}

impl Client {
    /// Returns standard DataStore operations.
    #[must_use]
    pub const fn data_stores(&self) -> DataStores<'_> {
        DataStores { client: self }
    }
}

impl DataStores<'_> {
    /// Lists standard DataStores in a universe.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn list(&self, universe_id: u64, options: &ListOptions) -> Result<DataStorePage> {
        let path = data_stores_path(universe_id);
        let builder = self.client.request(Method::GET, &path)?.query(options);
        self.client.send_json(builder).await
    }

    /// Deletes a standard DataStore.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn delete(&self, universe_id: u64, data_store_id: &str) -> Result<DataStore> {
        let path = item_path(&data_stores_path(universe_id), data_store_id);
        let builder = self.client.request(Method::DELETE, &path)?;
        self.client.send_json(builder).await
    }

    /// Restores a deleted standard DataStore.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn undelete(&self, universe_id: u64, data_store_id: &str) -> Result<DataStore> {
        let path = format!(
            "{}:undelete",
            item_path(&data_stores_path(universe_id), data_store_id)
        );
        let builder = self
            .client
            .request(Method::POST, &path)?
            .json(&serde_json::json!({}));
        self.client.send_json(builder).await
    }

    /// Lists entries in a standard DataStore.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn list_entries(
        &self,
        universe_id: u64,
        data_store_id: &str,
        scope: Option<&str>,
        options: &ListOptions,
    ) -> Result<EntryPage> {
        let path = entries_path(universe_id, data_store_id, scope);
        let builder = self.client.request(Method::GET, &path)?.query(options);
        self.client.send_json(builder).await
    }

    /// Returns an entry's JSON value.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn entry(
        &self,
        universe_id: u64,
        data_store_id: &str,
        entry_id: &str,
        scope: Option<&str>,
    ) -> Result<Value> {
        let (value, _) = self
            .entry_with_revision(universe_id, data_store_id, entry_id, scope)
            .await?;
        Ok(value)
    }

    /// Returns an entry's JSON value and current revision.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn entry_with_revision(
        &self,
        universe_id: u64,
        data_store_id: &str,
        entry_id: &str,
        scope: Option<&str>,
    ) -> Result<(Value, Option<String>)> {
        let path = item_path(&entries_path(universe_id, data_store_id, scope), entry_id);
        let builder = self.client.request(Method::GET, &path)?;
        let entry: Value = self.client.send_json(builder).await?;

        if let Value::Object(mut object) = entry {
            let revision = extract_revision(&object);
            if let Some(value) = object.remove("value") {
                return Ok((value, revision));
            }
            return Ok((Value::Object(object), revision));
        }

        Ok((entry, None))
    }

    /// Creates an entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn create_entry(
        &self,
        universe_id: u64,
        data_store_id: &str,
        entry_id: &str,
        scope: Option<&str>,
        value: &Value,
    ) -> Result<()> {
        let path = entries_path(universe_id, data_store_id, scope);
        let builder = self
            .client
            .request(Method::POST, &path)?
            .query(&[("id", entry_id)])
            .json(&serde_json::json!({ "value": value }));
        let _response = self.client.send(builder).await?;
        Ok(())
    }

    /// Creates or updates an entry.
    ///
    /// `match_version` enables an optimistic concurrency check.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn set_entry(
        &self,
        universe_id: u64,
        data_store_id: &str,
        entry_id: &str,
        scope: Option<&str>,
        value: &Value,
        match_version: Option<&str>,
    ) -> Result<Option<String>> {
        let path = item_path(&entries_path(universe_id, data_store_id, scope), entry_id);
        let mut query = Vec::with_capacity(2);
        query.push((String::from("allowMissing"), String::from("true")));
        if let Some(match_version) = match_version {
            query.push((String::from("matchVersion"), String::from(match_version)));
        }

        let builder = self
            .client
            .request(Method::PATCH, &path)?
            .query(&query)
            .json(&serde_json::json!({ "value": value }));
        let response: Value = self.client.send_json(builder).await?;
        Ok(response.as_object().and_then(extract_revision))
    }

    /// Deletes an entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn delete_entry(
        &self,
        universe_id: u64,
        data_store_id: &str,
        entry_id: &str,
        scope: Option<&str>,
    ) -> Result<()> {
        let path = item_path(&entries_path(universe_id, data_store_id, scope), entry_id);
        let builder = self.client.request(Method::DELETE, &path)?;
        let _response = self.client.send(builder).await?;
        Ok(())
    }
}

fn data_stores_path(universe_id: u64) -> String {
    universe_path(universe_id, "/data-stores")
}

fn entries_path(universe_id: u64, data_store_id: &str, scope: Option<&str>) -> String {
    let data_store_path = item_path(&data_stores_path(universe_id), data_store_id);
    scope.map_or_else(
        || format!("{data_store_path}/entries"),
        |scope| {
            format!(
                "{data_store_path}/scopes/{}/entries",
                encode_path_segment(scope)
            )
        },
    )
}

include!("generated/datastore.rs");
