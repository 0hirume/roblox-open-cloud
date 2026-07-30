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
use roblox_open_cloud::{Client, universe::GetUniverseRequest};

# async fn run() -> roblox_open_cloud::Result<()> {
let client = Client::api_key("your-api-key")?;
let response = client
    .universes()
    .get_universe(GetUniverseRequest::new("123"))
    .await?;

println!("{:?}", response.body().display_name);
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

Every recommended resource operation follows the same typed shape: select a
domain from `Client`, call an operation with its generated request struct, and
receive a `Response<T>`. Operation request types live in their domain modules,
while reusable OpenAPI models live in `roblox_open_cloud::types`.

```rust,no_run
use roblox_open_cloud::{
    Client,
    analytics::{
        QueriesDimensionValuesForAUniverseBody, QueriesDimensionValuesForAUniverseRequest,
    },
};

# async fn run() -> roblox_open_cloud::Result<()> {
let client = Client::api_key("your-api-key")?;
let response = client
    .analytics()
    .queries_dimension_values_for_a_universe(
        QueriesDimensionValuesForAUniverseRequest::new(
            123,
            QueriesDimensionValuesForAUniverseBody::new(
                "2026-07-31T01:00:00Z",
                "DailyActiveUsers",
                "2026-07-31T00:00:00Z",
            )
            .limit(100),
        ),
    )
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

## Code generation

The checked-in endpoint inventory is maintained by an unpublished Rust
workspace tool. Its OpenAPI input is vendored at `spec/openapi.json`, so
generation does not require a separate Creator Docs checkout:

```text
cargo run -p codegen -- check
cargo run -p codegen -- sync
```

`check` fails when the checked-in coverage inventory or generated Rust differs
from fresh output; `sync` rewrites the output and runs rustfmt. The vendored
specification and coverage inventory are excluded from the published crate;
their attribution and license remain included.

To import a newer snapshot from an explicit Creator Docs checkout, run:

```text
cargo run -p codegen -- update <creator-docs-directory>
```

A daily GitHub workflow performs that import and opens or updates a review PR
whenever the specification changes. Normal generation never reads the external
checkout.

## License

MIT
