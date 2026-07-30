//! MemoryStore operations.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{item_path, universe_path};
use crate::{Client, ListOptions, Result};

/// An item in a MemoryStore sorted map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortedMapItem {
    /// The item identifier.
    #[serde(default)]
    pub id: String,
    /// The stored JSON value.
    #[serde(default)]
    pub value: Value,
    /// The entity tag used for conditional updates.
    #[serde(default)]
    pub etag: Option<String>,
    /// The expiration timestamp.
    #[serde(default)]
    pub expire_time: Option<String>,
}

include!("generated/memory_store.rs");

/// A page of MemoryStore sorted-map items.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortedMapItemPage {
    /// The returned items.
    #[serde(default)]
    pub items: Vec<SortedMapItem>,
    /// A continuation token for the next page.
    #[serde(default)]
    pub next_page_token: Option<String>,
}

/// An item in a MemoryStore queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    /// The item identifier.
    #[serde(default)]
    pub id: String,
    /// The stored JSON value.
    #[serde(default)]
    pub value: Value,
    /// The queue priority.
    #[serde(default)]
    pub priority: Option<i64>,
}

/// Items read from a MemoryStore queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItems {
    /// Identifies this read for a later [`MemoryStore::discard_queue_items`] call.
    #[serde(default)]
    pub read_id: String,
    /// The returned items.
    #[serde(default)]
    pub items: Vec<QueueItem>,
}

#[derive(Debug, Serialize)]
struct SortedMapItemRequest<'value> {
    value: &'value Value,
    ttl: String,
}

#[derive(Debug, Serialize)]
struct QueueItemRequest<'value> {
    value: &'value Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<i64>,
    ttl: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscardQueueItemsRequest<'read> {
    read_id: &'read str,
}

/// MemoryStore operations for an authenticated client.
#[derive(Debug, Clone, Copy)]
pub struct MemoryStore<'client> {
    client: &'client Client,
}

impl Client {
    /// Returns MemoryStore operations.
    #[must_use]
    pub const fn memory_store(&self) -> MemoryStore<'_> {
        MemoryStore { client: self }
    }
}

impl MemoryStore<'_> {
    /// Lists sorted-map items.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn list_sorted_map_items(
        &self,
        universe_id: u64,
        sorted_map_id: &str,
        options: &ListOptions,
    ) -> Result<SortedMapItemPage> {
        let path = sorted_map_items_path(universe_id, sorted_map_id);
        let builder = self.client.request(Method::GET, &path)?.query(options);
        self.client.send_json(builder).await
    }

    /// Returns a sorted-map item.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn sorted_map_item(
        &self,
        universe_id: u64,
        sorted_map_id: &str,
        item_id: &str,
    ) -> Result<SortedMapItem> {
        let path = item_path(&sorted_map_items_path(universe_id, sorted_map_id), item_id);
        let builder = self.client.request(Method::GET, &path)?;
        self.client.send_json(builder).await
    }

    /// Creates a sorted-map item.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn create_sorted_map_item(
        &self,
        universe_id: u64,
        sorted_map_id: &str,
        item_id: &str,
        value: &Value,
        ttl_seconds: u64,
    ) -> Result<SortedMapItem> {
        let path = sorted_map_items_path(universe_id, sorted_map_id);
        let builder = self
            .client
            .request(Method::POST, &path)?
            .query(&[("id", item_id)])
            .json(&SortedMapItemRequest {
                value,
                ttl: format!("{ttl_seconds}s"),
            });
        self.client.send_json(builder).await
    }

    /// Updates a sorted-map item.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn update_sorted_map_item(
        &self,
        universe_id: u64,
        sorted_map_id: &str,
        item_id: &str,
        value: &Value,
        ttl_seconds: u64,
        etag: Option<&str>,
    ) -> Result<SortedMapItem> {
        let path = item_path(&sorted_map_items_path(universe_id, sorted_map_id), item_id);
        let query = etag.map_or_else(Vec::new, |etag| {
            vec![(String::from("etag"), String::from(etag))]
        });
        let builder = self
            .client
            .request(Method::PATCH, &path)?
            .query(&query)
            .json(&SortedMapItemRequest {
                value,
                ttl: format!("{ttl_seconds}s"),
            });
        self.client.send_json(builder).await
    }

    /// Deletes a sorted-map item.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn delete_sorted_map_item(
        &self,
        universe_id: u64,
        sorted_map_id: &str,
        item_id: &str,
    ) -> Result<()> {
        let path = item_path(&sorted_map_items_path(universe_id, sorted_map_id), item_id);
        let builder = self.client.request(Method::DELETE, &path)?;
        let _response = self.client.send(builder).await?;
        Ok(())
    }

    /// Adds an item to a queue.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn add_queue_item(
        &self,
        universe_id: u64,
        queue_id: &str,
        item_id: Option<&str>,
        value: &Value,
        priority: Option<i64>,
        ttl_seconds: u64,
    ) -> Result<QueueItem> {
        let path = queue_items_path(universe_id, queue_id);
        let mut builder = self.client.request(Method::POST, &path)?;
        if let Some(item_id) = item_id {
            builder = builder.query(&[("id", item_id)]);
        }
        let builder = builder.json(&QueueItemRequest {
            value,
            priority,
            ttl: format!("{ttl_seconds}s"),
        });
        self.client.send_json(builder).await
    }

    /// Reads items from a queue.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn read_queue_items(
        &self,
        universe_id: u64,
        queue_id: &str,
        count: u32,
        invisibility_window_seconds: u32,
        all_or_nothing: bool,
    ) -> Result<QueueItems> {
        let path = queue_items_path(universe_id, queue_id);
        let query = [
            (String::from("count"), count.to_string()),
            (
                String::from("invisibilityWindowSeconds"),
                invisibility_window_seconds.to_string(),
            ),
            (String::from("allOrNothing"), all_or_nothing.to_string()),
        ];
        let builder = self.client.request(Method::GET, &path)?.query(&query);
        self.client.send_json(builder).await
    }

    /// Discards the items returned by a queue read.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails or Roblox rejects it.
    pub async fn discard_queue_items(
        &self,
        universe_id: u64,
        queue_id: &str,
        read_id: &str,
    ) -> Result<()> {
        let path = format!("{}:discard", queue_items_path(universe_id, queue_id));
        let builder = self
            .client
            .request(Method::POST, &path)?
            .json(&DiscardQueueItemsRequest { read_id });
        let _response = self.client.send(builder).await?;
        Ok(())
    }
}

fn sorted_map_items_path(universe_id: u64, sorted_map_id: &str) -> String {
    format!(
        "{}/items",
        item_path(
            &universe_path(universe_id, "/memory-store/sorted-maps"),
            sorted_map_id
        )
    )
}

fn queue_items_path(universe_id: u64, queue_id: &str) -> String {
    format!(
        "{}/items",
        item_path(
            &universe_path(universe_id, "/memory-store/queues"),
            queue_id
        )
    )
}
