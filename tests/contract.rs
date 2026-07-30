use bytes as _;
use oauth2 as _;
use percent_encoding as _;
use reqwest as _;
use roblox_open_cloud::coverage;
use roblox_open_cloud::{Client, Error, ListOptions, Result};
use serde as _;
use serde_json::json;
use thiserror as _;
use url as _;
use wiremock::matchers::{body_json, body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn generated_inventory_covers_the_recommended_creator_docs_surface() -> Result<()> {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../spec/coverage.json"))?;
    let operations = manifest
        .get("operations")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::OAuth(String::from("coverage operations are missing")))?;

    assert_eq!(
        manifest.get("count").and_then(serde_json::Value::as_u64),
        Some(259)
    );
    assert_eq!(operations.len(), 259);
    assert_eq!(coverage::ENDPOINTS.len(), 259);

    let mut endpoint_keys = std::collections::BTreeSet::new();
    for endpoint in coverage::ENDPOINTS {
        assert!(
            endpoint_keys.insert(format!("{:?}:{}", endpoint.method(), endpoint.path())),
            "duplicate endpoint: {}",
            endpoint.path()
        );
        let auth = endpoint.authentication();
        assert!(auth.api_key() || auth.oauth() || auth.unauthenticated());
    }
    Ok(())
}

#[tokio::test]
async fn oauth_authorization_and_relay_token_lifecycle() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/v1/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=returned-code"))
        .and(body_string_contains("client_id=client-id"))
        .and(body_string_contains("client_secret=client-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access",
            "refresh_token": "refresh",
            "id_token": "identity",
            "token_type": "Bearer",
            "expires_in": 900,
            "scope": "openid profile"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/v1/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=refresh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "refreshed-access",
            "refresh_token": "refreshed-refresh",
            "token_type": "Bearer",
            "expires_in": 900,
            "scope": "openid profile"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/v1/token/introspect"))
        .and(body_string_contains("token=access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "active": true,
            "client_id": "client-id",
            "scope": "openid profile"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/v1/token/resources"))
        .and(body_string_contains("token=access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resource_infos": [{
                "owner": { "id": "7", "type": "User" },
                "resources": {
                    "universe": { "ids": ["42", "7", "42"] }
                }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/v1/token/revoke"))
        .and(body_string_contains("token=refresh"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let oauth = roblox_open_cloud::oauth::Client::new(
        "client-id",
        Some(String::from("client-secret")),
        "https://example.com/callback",
    )?
    .with_relay(&server.uri())?;
    let authorization = oauth.authorize(["openid", "profile"]);
    let query = authorization
        .url()
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(query.get("client_id").map(AsRef::as_ref), Some("client-id"));
    assert_eq!(
        query.get("code_challenge_method").map(AsRef::as_ref),
        Some("S256")
    );
    assert!(query.contains_key("code_challenge"));

    let state = String::from(authorization.state());
    let token = oauth
        .exchange("returned-code", &state, authorization)
        .await?;
    assert_eq!(token.access_token(), "access");
    assert_eq!(token.refresh_token(), Some("refresh"));
    assert_eq!(token.scopes(), ["openid", "profile"]);
    let refreshed = oauth.refresh("refresh").await?;
    assert_eq!(refreshed.access_token(), "refreshed-access");
    assert_eq!(refreshed.refresh_token(), Some("refreshed-refresh"));

    let info = oauth.introspect(token.access_token()).await?;
    assert!(info.active);
    assert_eq!(info.scopes(), ["openid", "profile"]);

    let resources = oauth.resources(token.access_token()).await?;
    assert_eq!(resources.universe_ids(), [7, 42]);
    oauth.revoke("refresh").await?;
    Ok(())
}

#[tokio::test]
async fn generated_operation_builders_apply_auth_query_and_json() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/analytics-query-api/v1/universes/42/dimension-values",
        ))
        .and(header("x-api-key", "key"))
        .and(query_param("limit", "5"))
        .and(body_json(json!({ "metric": "DailyActiveUsers" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "operation": "queued"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let response = client
        .analytics()
        .queries_dimension_values_for_a_universe(42)?
        .query("limit", 5)
        .json(&json!({ "metric": "DailyActiveUsers" }))?
        .send()
        .await?;

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn api_key_authenticates_universe_requests() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cloud/v2/universes/42"))
        .and(header("x-api-key", "key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "displayName": "Example" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let universe = client.universes().get(42).await?;

    assert_eq!(universe.display_name, "Example");
    Ok(())
}

#[tokio::test]
async fn oauth_authenticates_open_cloud_requests() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cloud/v2/universes/7"))
        .and(header("authorization", "Bearer token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "displayName": "OAuth" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::oauth("token")?.with_base_url(server.uri())?;
    let universe = client.universes().get(7).await?;

    assert_eq!(universe.display_name, "OAuth");
    Ok(())
}

#[tokio::test]
async fn datastore_update_preserves_path_query_body_and_revision() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(
            "/cloud/v2/universes/1/data-stores/players%2Fv2/scopes/global%20scope/entries/user%2F1",
        ))
        .and(query_param("allowMissing", "true"))
        .and(query_param("matchVersion", "revision-1"))
        .and(body_json(json!({ "value": { "coins": 10 } })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "revisionId": "revision-2" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let revision = client
        .data_stores()
        .set_entry(
            1,
            "players/v2",
            "user/1",
            Some("global scope"),
            &json!({ "coins": 10 }),
            Some("revision-1"),
        )
        .await?;

    assert_eq!(revision.as_deref(), Some("revision-2"));
    Ok(())
}

#[tokio::test]
async fn datastore_entry_unwraps_value_and_etag() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/cloud/v2/universes/1/data-stores/players/entries/user",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "value": [1, 2], "etag": "etag-1" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let (value, revision) = client
        .data_stores()
        .entry_with_revision(1, "players", "user", None)
        .await?;

    assert_eq!(value, json!([1, 2]));
    assert_eq!(revision.as_deref(), Some("etag-1"));
    Ok(())
}

#[tokio::test]
async fn ordered_datastore_increment_uses_action_endpoint() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/cloud/v2/universes/1/ordered-data-stores/scores/scopes/global/entries/player:increment",
        ))
        .and(body_json(json!({ "amount": 5.0 })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "id": "player", "value": 15.0 })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let entry = client
        .ordered_data_stores()
        .increment(1, "scores", "global", "player", 5.0)
        .await?;

    assert_eq!(entry.value.to_bits(), 15.0_f64.to_bits());
    Ok(())
}

#[tokio::test]
async fn memory_store_serializes_ttl_and_conditional_etag() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(
            "/cloud/v2/universes/1/memory-store/sorted-maps/players/items/user",
        ))
        .and(query_param("etag", "etag-1"))
        .and(body_json(json!({ "value": true, "ttl": "60s" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "user",
            "value": true,
            "etag": "etag-2"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let item = client
        .memory_store()
        .update_sorted_map_item(1, "players", "user", &json!(true), 60, Some("etag-1"))
        .await?;

    assert_eq!(item.etag.as_deref(), Some("etag-2"));
    Ok(())
}

#[tokio::test]
async fn list_options_use_documented_camel_case_names() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cloud/v2/universes/1/data-stores"))
        .and(query_param("filter", "state=ACTIVE"))
        .and(query_param("orderBy", "createTime desc"))
        .and(query_param("pageToken", "next"))
        .and(query_param("maxPageSize", "25"))
        .and(query_param("showDeleted", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "dataStores": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let options = ListOptions {
        filter: Some(String::from("state=ACTIVE")),
        order_by: Some(String::from("createTime desc")),
        page_token: Some(String::from("next")),
        max_page_size: Some(25),
        show_deleted: true,
    };
    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let page = client.data_stores().list(1, &options).await?;

    assert!(page.data_stores.is_empty(), "expected an empty page");
    Ok(())
}

#[tokio::test]
async fn messaging_accepts_oauth_as_documented() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cloud/v2/universes/1:publishMessage"))
        .and(header("authorization", "Bearer token"))
        .and(body_json(json!({
            "topic": "topic",
            "message": "message"
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::oauth("token")?.with_base_url(server.uri())?;
    client.messaging().publish(1, "topic", "message").await?;
    Ok(())
}

#[tokio::test]
async fn memory_store_discards_a_queue_read() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/cloud/v2/universes/1/memory-store/queues/jobs/items:discard",
        ))
        .and(header("x-api-key", "key"))
        .and(body_json(json!({ "readId": "read-1" })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    client
        .memory_store()
        .discard_queue_items(1, "jobs", "read-1")
        .await?;
    Ok(())
}

#[tokio::test]
async fn api_key_introspection_uses_the_unauthed_body_endpoint() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api-keys/v1/introspect"))
        .and(body_json(json!({ "apiKey": "key" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "deployment",
            "authorizedUserId": 123,
            "scopes": [{
                "name": "asset",
                "operations": ["write"],
                "groupIds": ["*"]
            }],
            "enabled": true,
            "expired": false
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let info = client.introspect_api_key().await?;

    assert_eq!(info.name, "deployment");
    let scope = info
        .scopes
        .first()
        .ok_or_else(|| Error::OAuth(String::from("introspection scope is missing")))?;
    assert_eq!(scope.group_ids, ["*"]);
    Ok(())
}

#[tokio::test]
async fn api_errors_preserve_status_and_body() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cloud/v2/universes/404"))
        .respond_with(ResponseTemplate::new(404).set_body_string("missing"))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let result = client.universes().get(404).await;

    match result {
        Err(Error::Api { status, body }) => {
            assert_eq!(status.as_u16(), 404);
            assert_eq!(body, "missing");
        }
        other => {
            assert!(
                other.is_err(),
                "expected an API error, received a successful response"
            );
        }
    }

    Ok(())
}
