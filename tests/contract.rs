use bytes as _;
use oauth2 as _;
use percent_encoding as _;
use reqwest as _;
use roblox_open_cloud::coverage;
use roblox_open_cloud::{Client, Error, Result};
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

    let operation_count = manifest
        .get("count")
        .and_then(serde_json::Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| Error::OAuth(String::from("coverage count is missing or invalid")))?;

    assert_eq!(operations.len(), operation_count);
    assert_eq!(coverage::ENDPOINTS.len(), operation_count);

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
        .and(body_json(json!({
            "endTime": "2026-07-31T01:00:00Z",
            "limit": 5,
            "metric": "DailyActiveUsers",
            "startTime": "2026-07-31T00:00:00Z"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "operation": "queued"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let response = client
        .analytics()
        .queries_dimension_values_for_a_universe(
            roblox_open_cloud::analytics::QueriesDimensionValuesForAUniverseRequest::new(
                42,
                roblox_open_cloud::analytics::QueriesDimensionValuesForAUniverseBody::new(
                    "2026-07-31T01:00:00Z",
                    "DailyActiveUsers",
                    "2026-07-31T00:00:00Z",
                )
                .limit(5),
            ),
        )
        .await?;

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(matches!(
        response.body(),
        roblox_open_cloud::analytics::AnalyticsQueriesDimensionValuesForAUniverseResponseBody::Ok(
            _
        )
    ));
    Ok(())
}

#[tokio::test]
async fn generated_multipart_request_serializes_text_and_file_fields() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/developer-products/v2/universes/42/developer-products",
        ))
        .and(header("x-api-key", "key"))
        .and(body_string_contains("name=\"name\""))
        .and(body_string_contains("Example Product"))
        .and(body_string_contains("filename=\"icon.png\""))
        .and(body_string_contains("icon-bytes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "createdTimestamp": "2026-07-31T00:00:00Z",
            "description": "Example",
            "isForSale": true,
            "isImmutable": false,
            "isManagedPricingEnabled": false,
            "name": "Example Product",
            "productId": 7,
            "universeId": 42,
            "updatedTimestamp": "2026-07-31T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let product = client
        .developer_products()
        .create_developer_product(
            roblox_open_cloud::developer_products::CreateDeveloperProductRequest::new(42).body(
                roblox_open_cloud::developer_products::CreateDeveloperProductBody::new(
                    "Example Product",
                )
                .description("Example")
                .image_file(
                    roblox_open_cloud::File::new("icon-bytes")
                        .with_name("icon.png")
                        .with_content_type("image/png"),
                )
                .is_for_sale(true)
                .is_managed_pricing_enabled(false),
            ),
        )
        .await?
        .into_body();

    assert_eq!(product.product_id, 7);
    Ok(())
}

#[tokio::test]
async fn api_key_authenticates_universe_requests() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cloud/v2/universes/42"))
        .and(header("x-api-key", "key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "displayName": "Example",
            "templateRootPlace": "places/1"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let universe = client
        .universes()
        .get_universe(roblox_open_cloud::universe::GetUniverseRequest::new("42"))
        .await?
        .into_body();

    assert_eq!(universe.display_name.as_deref(), Some("Example"));
    Ok(())
}

#[tokio::test]
async fn oauth_authenticates_open_cloud_requests() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cloud/v2/universes/7"))
        .and(header("authorization", "Bearer token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "displayName": "OAuth",
            "templateRootPlace": "places/1"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::oauth("token")?.with_base_url(server.uri())?;
    let universe = client
        .universes()
        .get_universe(roblox_open_cloud::universe::GetUniverseRequest::new("7"))
        .await?
        .into_body();

    assert_eq!(universe.display_name.as_deref(), Some("OAuth"));
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
        .and(body_json(json!({
            "etag": "revision-1",
            "value": { "coins": 10 }
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "revisionId": "revision-2" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let entry = client
        .datastore()
        .patch_scope_id_entries_entry_id(
            roblox_open_cloud::datastore::PatchScopeIdEntriesEntryIdRequest::new(
                "players/v2",
                "user/1",
                "global scope",
                "1",
                roblox_open_cloud::datastore::PatchScopeIdEntriesEntryIdBody::new()
                    .etag("revision-1")
                    .value(json!({ "coins": 10 })),
            )
            .allow_missing(true),
        )
        .await?
        .into_body();

    assert_eq!(entry.revision_id.as_deref(), Some("revision-2"));
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
    let entry = client
        .datastore()
        .get_data_store_id_entries_entry_id(
            roblox_open_cloud::datastore::GetDataStoreIdEntriesEntryIdRequest::new(
                "players", "user", "1",
            ),
        )
        .await?
        .into_body();

    assert_eq!(entry.value, Some(json!([1, 2])));
    assert_eq!(entry.etag.as_deref(), Some("etag-1"));
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
        .ordered_datastore()
        .increment_ordered_data_store_entry(
            roblox_open_cloud::ordered_datastore::IncrementOrderedDataStoreEntryRequest::new(
                "player",
                "scores",
                "global",
                "1",
                roblox_open_cloud::ordered_datastore::IncrementOrderedDataStoreEntryBody::new()
                    .amount(5.0),
            ),
        )
        .await?
        .into_body();

    assert_eq!(entry.value.map(f64::to_bits), Some(15.0_f64.to_bits()));
    Ok(())
}

#[tokio::test]
async fn memory_store_serializes_ttl_and_conditional_etag() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(
            "/cloud/v2/universes/1/memory-store/sorted-maps/players/items/user",
        ))
        .and(body_json(json!({
            "etag": "etag-1",
            "ttl": "60s",
            "value": true
        })))
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
        .update_memory_store_sorted_map_item(
            roblox_open_cloud::memory_store::UpdateMemoryStoreSortedMapItemRequest::new(
                "user",
                "players",
                "1",
                roblox_open_cloud::memory_store::UpdateMemoryStoreSortedMapItemBody::new()
                    .etag("etag-1")
                    .ttl("60s")
                    .value(json!(true)),
            ),
        )
        .await?
        .into_body();

    assert_eq!(item.etag.as_deref(), Some("etag-2"));
    Ok(())
}

#[tokio::test]
async fn generated_list_request_uses_documented_camel_case_names() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cloud/v2/universes/1/data-stores"))
        .and(query_param("filter", "state=ACTIVE"))
        .and(query_param("pageToken", "next"))
        .and(query_param("maxPageSize", "25"))
        .and(query_param("showDeleted", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "dataStores": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::api_key("key")?.with_base_url(server.uri())?;
    let page = client
        .datastore()
        .list_data_stores(
            roblox_open_cloud::datastore::ListDataStoresRequest::new("1")
                .filter("state=ACTIVE")
                .max_page_size(25)
                .page_token("next")
                .show_deleted(true),
        )
        .await?
        .into_body();

    assert!(
        page.data_stores.unwrap_or_default().is_empty(),
        "expected an empty page"
    );
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
    let _response = client
        .messaging()
        .publish_universe_message(
            roblox_open_cloud::messaging::PublishUniverseMessageRequest::new(
                "1",
                roblox_open_cloud::messaging::PublishUniverseMessageBody::new("message", "topic"),
            ),
        )
        .await?;
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
    let _response = client
        .memory_store()
        .discard_memory_store_queue_items(
            roblox_open_cloud::memory_store::DiscardMemoryStoreQueueItemsRequest::new(
                "jobs",
                "1",
                roblox_open_cloud::memory_store::DiscardMemoryStoreQueueItemsBody::new()
                    .read_id("read-1"),
            ),
        )
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
    let result = client
        .universes()
        .get_universe(roblox_open_cloud::universe::GetUniverseRequest::new("404"))
        .await;

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
