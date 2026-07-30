//! Async access to Roblox Open Cloud.
//!
//! The crate exposes one authenticated [`Client`] and domain-specific API
//! handles. It contains no command-line, browser, persistence, or scaffolding
//! policy.

pub mod auth;
mod client;
mod error;
mod operation;

#[cfg(test)]
use tokio as _;
#[cfg(test)]
use wiremock as _;

#[path = "generated/analytics.rs"]
pub mod analytics;
#[path = "generated/assets.rs"]
pub mod assets;
#[path = "generated/avatars.rs"]
pub mod avatars;
#[path = "generated/badges.rs"]
pub mod badges;
#[path = "generated/configs.rs"]
pub mod configs;
#[path = "generated/coverage.rs"]
pub mod coverage;
#[path = "generated/creator_store.rs"]
pub mod creator_store;
pub mod datastore;
#[path = "generated/developer_products.rs"]
pub mod developer_products;
#[path = "generated/game_passes.rs"]
pub mod game_passes;
#[path = "generated/generative_ai.rs"]
pub mod generative_ai;
#[path = "generated/groups.rs"]
pub mod groups;
#[path = "generated/interactions.rs"]
pub mod interactions;
#[path = "generated/inventory.rs"]
pub mod inventory;
#[path = "generated/localization.rs"]
pub mod localization;
#[path = "generated/luau.rs"]
pub mod luau;
#[path = "generated/matchmaking.rs"]
pub mod matchmaking;
pub mod memory_store;
pub mod messaging;
#[path = "generated/notifications.rs"]
pub mod notifications;
pub mod oauth;
pub mod ordered_datastore;
#[path = "generated/places.rs"]
pub mod places;
#[path = "generated/restrictions.rs"]
pub mod restrictions;
#[path = "generated/secrets.rs"]
pub mod secrets;
#[path = "generated/subscriptions.rs"]
pub mod subscriptions;
#[path = "generated/team_create.rs"]
pub mod team_create;
#[path = "generated/thumbnails.rs"]
pub mod thumbnails;
pub mod universe;
#[path = "generated/users.rs"]
pub mod users;

pub use auth::{Authentication, Credentials};
pub use client::{Client, ListOptions, RawResponse};
pub use error::{Error, Result};
pub use operation::{AuthenticationSupport, Endpoint, HttpMethod, OperationRequest, Stability};
