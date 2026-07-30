# roblox-open-cloud

`roblox-open-cloud` is an async Rust library for Roblox Open Cloud. It supports
API keys, OAuth 2.0 with PKCE, and the recommended stable and beta API surface
published in Roblox Creator Docs.

The crate deliberately excludes command-line parsing, terminal interfaces,
browser launching, token persistence, project scaffolding, and application-
specific OAuth relays.

The initial release covers 259 resource operations across 28 domains, plus
API-key introspection and the OAuth authorization, token, introspection,
resources, revocation, user-info, and discovery endpoints.

## Usage

```rust,no_run
use roblox_open_cloud::Client;

# async fn run() -> roblox_open_cloud::Result<()> {
let client = Client::api_key("your-api-key")?;
let universe = client.universes().get(123).await?;
println!("{}", universe.display_name);
# Ok(())
# }
```

OAuth access tokens use the same API surface:

```rust,no_run
use roblox_open_cloud::Client;

# fn build() -> roblox_open_cloud::Result<()> {
let client = Client::oauth("access-token")?;
# Ok(())
# }
```

Universes, standard and ordered DataStores, MemoryStore, messaging, and the
OAuth lifecycle have focused typed APIs. The complete recommended surface is
available through domain request builders:

```rust,no_run
use roblox_open_cloud::Client;
use serde_json::json;

# async fn run() -> roblox_open_cloud::Result<()> {
let client = Client::api_key("your-api-key")?;
let response = client
    .analytics()
    .queries_dimension_values_for_a_universe(123)?
    .query("limit", 100)
    .json(&json!({ "metric": "DailyActiveUsers" }))?
    .send()
    .await?;

println!("{}", response.status());
# Ok(())
# }
```

Every operation exposes its HTTP method, path template, stability, accepted
authentication modes, and required scopes. The aggregate inventory is
available as `roblox_open_cloud::coverage::ENDPOINTS`.

## API policy

The supported surface is derived from the consolidated OpenAPI document in the
Roblox Creator Docs repository:

- Stable and beta operations are supported.
- Older operations are supported when they accept API keys or OAuth, which
  Creator Docs grants beta-level stability.
- Cookie-only, deprecated, and experimental operations are excluded.

See `spec/coverage.json` in the source repository for the pinned inventory.

## License

MIT
