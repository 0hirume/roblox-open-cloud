use std::collections::{BTreeMap, BTreeSet};

use heck::ToSnakeCase;
use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use syn::{Type, parse_quote};

use crate::schema::{BodyKind, OperationInput, ParameterLocation, ResponseFormat, TypedOperation};
use crate::{CodegenError, Result};

const RUST_KEYWORDS: &[&str] = &[
    "Self", "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
    "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "yield",
];

#[derive(Debug, Deserialize)]
struct OpenApi {
    paths: BTreeMap<String, PathItem>,
}

#[derive(Debug, Default, Deserialize)]
struct PathItem {
    delete: Option<ApiOperation>,
    get: Option<ApiOperation>,
    head: Option<ApiOperation>,
    options: Option<ApiOperation>,
    patch: Option<ApiOperation>,
    post: Option<ApiOperation>,
    put: Option<ApiOperation>,
    trace: Option<ApiOperation>,
}

impl PathItem {
    const fn operations(&self) -> [(&'static str, Option<&ApiOperation>); 8] {
        [
            ("delete", self.delete.as_ref()),
            ("get", self.get.as_ref()),
            ("head", self.head.as_ref()),
            ("options", self.options.as_ref()),
            ("patch", self.patch.as_ref()),
            ("post", self.post.as_ref()),
            ("put", self.put.as_ref()),
            ("trace", self.trace.as_ref()),
        ]
    }
}

#[derive(Debug, Deserialize)]
struct ApiOperation {
    #[serde(rename = "operationId")]
    operation_id: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    security: Vec<BTreeMap<String, Value>>,
    #[serde(rename = "x-roblox-stability")]
    stability: Option<String>,
    #[serde(default)]
    deprecated: bool,
    #[serde(rename = "x-roblox-deprecated")]
    roblox_deprecated: Option<Value>,
    #[serde(rename = "x-roblox-scopes")]
    scopes: Option<Vec<Scope>>,
}

#[derive(Debug, Deserialize)]
struct Scope {
    name: String,
}

#[derive(Debug)]
struct BaseOperation {
    domain: String,
    http_method: String,
    operation_id: String,
    path: String,
    path_parameters: Vec<String>,
    summary: String,
    tag: String,
    stability: String,
    api_key: bool,
    oauth: bool,
    unauthenticated: bool,
    scopes: Vec<String>,
    base_method: String,
}

#[derive(Debug, Serialize)]
struct Operation {
    domain: String,
    http_method: String,
    operation_id: String,
    path: String,
    path_parameters: Vec<String>,
    summary: String,
    tag: String,
    stability: String,
    api_key: bool,
    oauth: bool,
    unauthenticated: bool,
    scopes: Vec<String>,
    id: String,
    rust_method: String,
    constant: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Source {
    repository: String,
    commit: String,
    commit_date: String,
    document: String,
    vendored: String,
}

impl Source {
    pub(super) fn creator_docs(commit: String, commit_date: String) -> Self {
        Self {
            repository: String::from("Roblox/creator-docs"),
            commit,
            commit_date,
            document: String::from("content/en-us/reference/cloud/openapi.json"),
            vendored: String::from("spec/openapi.json"),
        }
    }
}

#[derive(Debug, Serialize)]
struct Policy {
    include: [&'static str; 2],
    exclude: [&'static str; 4],
}

#[derive(Debug, Serialize)]
struct Coverage<'source, 'operations> {
    source: &'source Source,
    policy: Policy,
    count: usize,
    operations: &'operations [Operation],
}

#[derive(Debug)]
pub(super) struct Generated {
    coverage_json: String,
    coverage_rust: String,
    domains: BTreeMap<String, String>,
    types: String,
}

impl Generated {
    pub(super) fn coverage_json(&self) -> &str {
        &self.coverage_json
    }

    pub(super) fn coverage_rust(&self) -> &str {
        &self.coverage_rust
    }

    pub(super) const fn domains(&self) -> &BTreeMap<String, String> {
        &self.domains
    }

    pub(super) fn types(&self) -> &str {
        &self.types
    }
}

pub(super) fn generate(api_bytes: &[u8], source: &Source) -> Result<Generated> {
    let api: OpenApi = serde_json::from_slice(api_bytes).map_err(|json_error| {
        Box::new(CodegenError::Json {
            path: "openapi.json".into(),
            source: json_error,
        })
    })?;
    let operations = collect_operations(&api)?;
    let api_value: Value = serde_json::from_slice(api_bytes).map_err(|json_error| {
        Box::new(CodegenError::Json {
            path: "openapi.json".into(),
            source: json_error,
        })
    })?;
    let operation_inputs: Vec<OperationInput> = operations
        .iter()
        .map(|operation| OperationInput {
            id: operation.id.clone(),
            domain: operation.domain.clone(),
            method: operation.http_method.clone(),
            path: operation.path.clone(),
            rust_method: operation.rust_method.clone(),
        })
        .collect();
    let typed = crate::schema::generate(&api_value, &operation_inputs)?;
    let mut grouped = BTreeMap::<String, Vec<&Operation>>::new();
    for operation in &operations {
        grouped
            .entry(String::from(operation.domain.as_str()))
            .or_default()
            .push(operation);
    }

    let mut domains = BTreeMap::new();
    for (domain, domain_operations) in grouped {
        let previous = domains.insert(
            String::from(domain.as_str()),
            render_domain(&domain, &domain_operations, &typed.operations)?,
        );
        if previous.is_some() {
            return Err(Box::new(CodegenError::Specification(format!(
                "generated domain `{domain}` more than once"
            ))));
        }
    }
    let coverage_rust = render_coverage(&operations)?;
    let coverage = Coverage {
        source,
        policy: Policy {
            include: [
                "non-deprecated, non-experimental operations supporting API keys or OAuth",
                "non-deprecated unauthenticated operations marked stable or beta",
            ],
            exclude: [
                "cookie-only operations",
                "deprecated operations",
                "experimental operations",
                "unauthenticated operations without a stability marker",
            ],
        },
        count: operations.len(),
        operations: &operations,
    };
    let coverage_json = serde_json::to_string_pretty(&coverage).map_err(|json_error| {
        Box::new(CodegenError::Json {
            path: "spec/coverage.json".into(),
            source: json_error,
        })
    })?;

    Ok(Generated {
        coverage_json,
        coverage_rust,
        domains,
        types: typed.types,
    })
}

fn collect_operations(api: &OpenApi) -> Result<Vec<Operation>> {
    let mut base_operations = Vec::new();
    for (path, path_item) in &api.paths {
        for (method, operation) in path_item.operations() {
            if let Some(operation) = operation
                && let Some(operation) = collect_operation(path, method, operation)?
            {
                base_operations.push(operation);
            }
        }
    }

    let mut groups = BTreeMap::<String, Vec<()>>::new();
    for operation in &base_operations {
        groups
            .entry(format!("{}::{}", operation.domain, operation.base_method))
            .or_default()
            .push(());
    }
    let duplicate_keys: BTreeSet<String> = groups
        .into_iter()
        .filter_map(|(key, operations)| (operations.len() > 1).then_some(key))
        .collect();

    let mut operations = Vec::with_capacity(base_operations.len());
    for operation in base_operations {
        let key = format!("{}::{}", operation.domain, operation.base_method);
        let rust_method = if !duplicate_keys.contains(&key) && operation.base_method.len() <= 72 {
            operation.base_method
        } else {
            route_identifier(&operation.http_method, &operation.path)
        };
        let constant = rust_method.to_uppercase();
        let id = format!("{}::{rust_method}", operation.domain);
        operations.push(Operation {
            domain: operation.domain,
            http_method: operation.http_method,
            operation_id: operation.operation_id,
            path: operation.path,
            path_parameters: operation.path_parameters,
            summary: operation.summary,
            tag: operation.tag,
            stability: operation.stability,
            api_key: operation.api_key,
            oauth: operation.oauth,
            unauthenticated: operation.unauthenticated,
            scopes: operation.scopes,
            id,
            rust_method,
            constant,
        });
    }
    operations.sort_by(|left, right| {
        left.domain
            .cmp(&right.domain)
            .then_with(|| left.rust_method.cmp(&right.rust_method))
    });

    let mut ids = BTreeMap::<String, Vec<()>>::new();
    for operation in &operations {
        ids.entry(String::from(operation.id.as_str()))
            .or_default()
            .push(());
    }
    let duplicate_ids: Vec<String> = ids
        .into_iter()
        .filter_map(|(id, matching)| (matching.len() > 1).then_some(id))
        .collect();
    if duplicate_ids.is_empty() {
        Ok(operations)
    } else {
        Err(Box::new(CodegenError::Specification(format!(
            "generated Rust endpoint identifiers are not unique: {}",
            duplicate_ids.join(", ")
        ))))
    }
}

fn collect_operation(
    path: &str,
    method: &str,
    operation: &ApiOperation,
) -> Result<Option<BaseOperation>> {
    let schemes: BTreeSet<&str> = operation
        .security
        .iter()
        .flat_map(|requirement| requirement.keys().map(String::as_str))
        .collect();
    let tag = operation
        .tags
        .first()
        .map_or("Uncategorized", String::as_str);
    let http_method = method.to_uppercase();
    let summary = operation
        .summary
        .as_ref()
        .map_or_else(|| format!("{http_method} {path}"), Clone::clone);
    let documented_stability = operation.stability.as_deref().unwrap_or("UNSPECIFIED");
    let deprecated = operation.deprecated || operation.roblox_deprecated.is_some();
    let api_key = schemes.contains("roblox-api-key");
    let oauth = schemes.contains("roblox-oauth2");
    let modern = api_key || oauth;
    let unauthenticated = schemes.is_empty();
    let recommended = !deprecated
        && ((modern && documented_stability != "EXPERIMENTAL")
            || (unauthenticated && matches!(documented_stability, "STABLE" | "BETA")));
    if !recommended {
        return Ok(None);
    }

    let stability = match documented_stability {
        "STABLE" => "stable",
        "BETA" => "beta",
        _ => "legacy-beta",
    };
    let scopes: Vec<String> = operation
        .scopes
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|scope| String::from(scope.name.as_str()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let domain = domain_for(tag, path, &summary)?;
    let base_method = rust_identifier(&summary);

    Ok(Some(BaseOperation {
        domain: String::from(domain),
        http_method,
        operation_id: operation.operation_id.clone().unwrap_or_default(),
        path: String::from(path),
        path_parameters: path_parameters(path),
        summary,
        tag: String::from(tag),
        stability: String::from(stability),
        api_key,
        oauth,
        unauthenticated,
        scopes,
        base_method,
    }))
}

pub(super) fn rust_identifier(value: &str) -> String {
    let snake = value.to_snake_case();
    let mut normalized = String::with_capacity(snake.len());
    let mut previous_separator = false;
    for character in snake.chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character);
            previous_separator = false;
        } else if !previous_separator {
            normalized.push('_');
            previous_separator = true;
        }
    }
    let mut identifier = String::from(normalized.trim_matches('_'));
    if identifier
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        identifier = format!("operation_{identifier}");
    } else if identifier.is_empty() {
        identifier = String::from("operation");
    }
    if RUST_KEYWORDS.contains(&identifier.as_str()) {
        format!("{identifier}_value")
    } else {
        identifier
    }
}

fn path_parameters(path: &str) -> Vec<String> {
    path.split('{')
        .skip(1)
        .filter_map(|segment| {
            segment
                .split_once('}')
                .map(|(name, _remainder)| String::from(name))
        })
        .collect()
}

fn route_identifier(method: &str, path: &str) -> String {
    let parts: Vec<String> = path
        .split('/')
        .filter(|part| !part.is_empty() && !is_version(part))
        .map(|part| {
            part.chars()
                .filter(|character| !matches!(character, '{' | '}'))
                .collect()
        })
        .collect();
    let tail: Vec<&str> = parts
        .iter()
        .skip(parts.len().saturating_sub(3))
        .map(String::as_str)
        .collect();
    let name = rust_identifier(&format!("{method} {}", tail.join(" ")));
    if name.len() <= 72 {
        name
    } else {
        let shorter: Vec<&str> = parts
            .iter()
            .skip(parts.len().saturating_sub(2))
            .map(String::as_str)
            .collect();
        rust_identifier(&format!("{method} {}", shorter.join(" ")))
    }
}

fn is_version(value: &str) -> bool {
    value.strip_prefix('v').is_some_and(|version| {
        !version.is_empty() && version.chars().all(|character| character.is_ascii_digit())
    })
}

fn domain_for(tag: &str, path: &str, summary: &str) -> Result<&'static str> {
    let lower_path = path.to_ascii_lowercase();
    let lower_summary = summary.to_ascii_lowercase();
    let domain = if lower_path.contains(":publishmessage") {
        "messaging"
    } else if lower_path.contains("/secrets") {
        "secrets"
    } else if lower_path.contains("ordered-data-store")
        || lower_summary.contains("ordered data store")
    {
        "ordered_datastore"
    } else if lower_path.contains("memory-store") || lower_summary.contains("memory store") {
        "memory_store"
    } else if lower_path.contains("data-store") || lower_path.contains("datastore") {
        "datastore"
    } else if lower_path.contains("user-restriction") {
        "restrictions"
    } else if lower_path.contains("luau-execution") {
        "luau"
    } else if lower_path.contains("/subscriptions/") {
        "subscriptions"
    } else {
        match tag {
            "Analytics" => "analytics",
            "Assets" => "assets",
            "Avatars" => "avatars",
            "Badges" => "badges",
            "Bans and blocks" => "restrictions",
            "Configs" => "configs",
            "Creator Store" => "creator_store",
            "Data and memory stores" => "datastore",
            "Developer products" => "developer_products",
            "Game passes" => "game_passes",
            "Generative AI" => "generative_ai",
            "Groups" => "groups",
            "Interactions" => "interactions",
            "Inventories" => "inventory",
            "Localization" => "localization",
            "Luau Execution" => "luau",
            "Matchmaking" => "matchmaking",
            "Notifications" => "notifications",
            "Places" => "places",
            "Team Create" => "team_create",
            "Thumbnails" => "thumbnails",
            "Universes" => "universe",
            "Users" => "users",
            _ => {
                return Err(Box::new(CodegenError::Specification(format!(
                    "no Rust domain mapping for OpenAPI tag `{tag}`"
                ))));
            }
        }
    };
    Ok(domain)
}

fn service_type(domain: &str) -> Result<&'static str> {
    match domain {
        "analytics" => Ok("Analytics"),
        "assets" => Ok("Assets"),
        "avatars" => Ok("Avatars"),
        "badges" => Ok("Badges"),
        "configs" => Ok("Configs"),
        "creator_store" => Ok("CreatorStore"),
        "datastore" => Ok("DataStores"),
        "developer_products" => Ok("DeveloperProducts"),
        "game_passes" => Ok("GamePasses"),
        "generative_ai" => Ok("GenerativeAi"),
        "groups" => Ok("Groups"),
        "interactions" => Ok("Interactions"),
        "inventory" => Ok("Inventory"),
        "localization" => Ok("Localization"),
        "luau" => Ok("Luau"),
        "matchmaking" => Ok("Matchmaking"),
        "memory_store" => Ok("MemoryStore"),
        "messaging" => Ok("Messaging"),
        "notifications" => Ok("Notifications"),
        "ordered_datastore" => Ok("OrderedDataStores"),
        "places" => Ok("Places"),
        "restrictions" => Ok("Restrictions"),
        "secrets" => Ok("Secrets"),
        "subscriptions" => Ok("Subscriptions"),
        "team_create" => Ok("TeamCreate"),
        "thumbnails" => Ok("Thumbnails"),
        "universe" => Ok("Universes"),
        "users" => Ok("Users"),
        _ => Err(Box::new(CodegenError::Specification(format!(
            "no service type for generated domain `{domain}`"
        )))),
    }
}

fn accessor_name(domain: &str) -> &str {
    if domain == "universe" {
        "universes"
    } else {
        domain
    }
}

fn http_variant(method: &str) -> Result<&'static str> {
    match method {
        "DELETE" => Ok("Delete"),
        "GET" => Ok("Get"),
        "HEAD" => Ok("Head"),
        "OPTIONS" => Ok("Options"),
        "PATCH" => Ok("Patch"),
        "POST" => Ok("Post"),
        "PUT" => Ok("Put"),
        "TRACE" => Ok("Trace"),
        _ => Err(Box::new(CodegenError::Specification(format!(
            "unsupported HTTP method `{method}`"
        )))),
    }
}

fn stability_variant(stability: &str) -> Result<&'static str> {
    match stability {
        "stable" => Ok("Stable"),
        "beta" => Ok("Beta"),
        "legacy-beta" => Ok("LegacyBeta"),
        _ => Err(Box::new(CodegenError::Specification(format!(
            "unsupported stability `{stability}`"
        )))),
    }
}

fn summary_doc(summary: &str) -> String {
    summary.replace(['\r', '\n'], " ")
}

fn render_endpoint(operation: &Operation) -> Result<TokenStream> {
    let constant = format_ident!("{}", operation.constant);
    let method = format_ident!("{}", http_variant(&operation.http_method)?);
    let stability = format_ident!("{}", stability_variant(&operation.stability)?);
    let doc = summary_doc(&operation.summary);
    let path = &operation.path;
    let summary = &operation.summary;
    let api_key = operation.api_key;
    let oauth = operation.oauth;
    let unauthenticated = operation.unauthenticated;
    let scopes = operation.scopes.iter().map(String::as_str);

    Ok(quote! {
        #[doc = #doc]
        pub const #constant: crate::Endpoint = crate::Endpoint::new(
            crate::HttpMethod::#method,
            #path,
            #summary,
            crate::Stability::#stability,
            crate::AuthenticationSupport::new(#api_key, #oauth, #unauthenticated),
            &[#(#scopes),*],
        );
    })
}

fn render_request(typed: &TypedOperation) -> TokenStream {
    let request_type = format_ident!("{}", typed.request_type);
    let request_doc = format!("Input for [`{}`].", typed.request_type);
    if typed.fields.is_empty() && typed.body.is_none() {
        return quote! {
            #[doc = #request_doc]
            #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
            pub struct #request_type;

            impl #request_type {
                /// Creates an empty request.
                #[must_use]
                pub const fn new() -> Self {
                    Self
                }
            }
        };
    }

    let mut fields: Vec<TokenStream> = typed
        .fields
        .iter()
        .map(|field| {
            let field_name = format_ident!("{}", field.rust_name);
            let rust_type = &field.rust_type;
            let field_type: Type = if field.optional {
                parse_quote!(Option<#rust_type>)
            } else {
                field.rust_type.clone()
            };
            let field_doc = format!("Value for `{}`.", field.wire_name);
            quote! {
                #[doc = #field_doc]
                pub #field_name: #field_type,
            }
        })
        .collect();
    if let Some(body) = &typed.body {
        let alias = request_body_type(typed, body);
        let body_type: Type = if body.optional {
            parse_quote!(Option<#alias>)
        } else {
            alias
        };
        fields.push(quote! {
            /// The request body.
            pub body: #body_type,
        });
    }

    let defaultable = typed.fields.iter().all(|field| field.optional)
        && typed.body.as_ref().is_none_or(|body| body.optional);
    let derive = if defaultable {
        quote!(#[derive(Debug, Clone, Default)])
    } else {
        quote!(#[derive(Debug, Clone)])
    };
    let request_impl = render_request_impl(typed);

    quote! {
        #[doc = #request_doc]
        #derive
        pub struct #request_type {
            #(#fields)*
        }

        #request_impl
    }
}

fn render_body_alias(typed: &TypedOperation) -> Option<TokenStream> {
    typed
        .body
        .as_ref()
        .zip(typed.body_alias.as_ref())
        .map(|(body, alias)| {
            let request_doc = format!("Request body for [`{}`].", typed.request_type);
            let alias = format_ident!("{alias}");
            let rust_type = &body.rust_type;
            quote! {
                #[doc = #request_doc]
                pub type #alias = #rust_type;
            }
        })
}

fn request_body_type(typed: &TypedOperation, body: &crate::schema::RequestBody) -> Type {
    typed.body_alias.as_ref().map_or_else(
        || body.rust_type.clone(),
        |alias| {
            let alias = format_ident!("{alias}");
            parse_quote!(#alias)
        },
    )
}

fn render_request_impl(typed: &TypedOperation) -> TokenStream {
    let request_type = format_ident!("{}", typed.request_type);
    let mut members: Vec<(proc_macro2::Ident, Type, bool)> = typed
        .fields
        .iter()
        .map(|field| {
            (
                format_ident!("{}", field.rust_name),
                field.rust_type.clone(),
                field.optional,
            )
        })
        .collect();
    if let Some(body) = &typed.body {
        members.push((
            format_ident!("body"),
            request_body_type(typed, body),
            body.optional,
        ));
    }
    let required: Vec<_> = members
        .iter()
        .filter(|(_, _, optional)| !optional)
        .collect();
    let parameters: Vec<_> = required
        .iter()
        .map(|(name, rust_type, _)| quote!(#name: impl Into<#rust_type>))
        .collect();
    let initializers: Vec<_> = members
        .iter()
        .map(|(name, _, optional)| {
            if *optional {
                quote!(#name: None,)
            } else {
                quote!(#name: #name.into(),)
            }
        })
        .collect();
    let constructor = if required.is_empty() {
        quote! {
            /// Creates a request from its required fields.
            #[must_use]
            pub const fn new() -> Self {
                Self {
                    #(#initializers)*
                }
            }
        }
    } else {
        quote! {
            /// Creates a request from its required fields.
            #[must_use]
            pub fn new(#(#parameters),*) -> Self {
                Self {
                    #(#initializers)*
                }
            }
        }
    };
    let setters = members
        .iter()
        .filter(|(_, _, optional)| *optional)
        .map(|(name, rust_type, _)| render_request_setter(name, rust_type))
        .collect::<Vec<_>>();

    quote! {
        impl #request_type {
            #constructor
            #(#setters)*
        }
    }
}

fn render_request_setter(name: &proc_macro2::Ident, rust_type: &Type) -> TokenStream {
    let doc = format!("Sets `{name}`.");
    quote! {
        #[doc = #doc]
        #[must_use]
        pub fn #name(mut self, #name: impl Into<#rust_type>) -> Self {
            self.#name = Some(#name.into());
            self
        }
    }
}

fn render_response(typed: &TypedOperation) -> TokenStream {
    let response_type = format_ident!("{}", typed.response_type);
    typed.response_body_type.as_ref().map_or_else(
        || {
            let rust_type = typed
                .responses
                .first()
                .map_or_else(|| parse_quote!(()), |response| response.rust_type.clone());
            quote! {
                /// Successful response from this operation.
                pub type #response_type = crate::Response<#rust_type>;
            }
        },
        |body_type| {
            let body_type = format_ident!("{body_type}");
            let body_doc = format!("Successful response body for [`{}`].", typed.response_type);
            let variants = typed.responses.iter().map(|response| {
                let variant = status_variant(response.status);
                let rust_type = &response.rust_type;
                if response.format == ResponseFormat::Empty {
                    quote!(#variant,)
                } else {
                    quote!(#variant(#rust_type),)
                }
            });

            quote! {
                #[doc = #body_doc]
                #[derive(Debug, Clone)]
                pub enum #body_type {
                    #(#variants)*
                }

                /// Successful response from this operation.
                pub type #response_type = crate::Response<#body_type>;
            }
        },
    )
}

fn render_method(operation: &Operation, typed: &TypedOperation) -> Result<TokenStream> {
    let summary = summary_doc(&operation.summary);
    let empty_request = typed.fields.is_empty() && typed.body.is_none();
    let request_name = format_ident!("{}", if empty_request { "_request" } else { "request" });
    let method = format_ident!("{}", operation.rust_method);
    let request_type = format_ident!("{}", typed.request_type);
    let response_type = format_ident!("{}", typed.response_type);
    let endpoint = format_ident!("{}", operation.constant);
    let parameter_calls = typed.fields.chunks(32).enumerate().map(|(index, _)| {
        let number = index.saturating_add(1);
        let helper = format_ident!("{}_parameters_{number}", operation.rust_method);
        quote! {
            let operation = #helper(operation, &request)?;
        }
    });
    let body = typed
        .body
        .as_ref()
        .map_or_else(TokenStream::new, render_body);
    let decode = render_decode(typed)?;

    Ok(quote! {
        #[doc = #summary]
        ///
        /// # Errors
        ///
        /// Returns an error if the request fails or its response is invalid.
        pub async fn #method(
            &self,
            #request_name: #request_type,
        ) -> crate::Result<#response_type> {
            let operation = self.client.operation(#endpoint);
            #(#parameter_calls)*
            #body
            let response = operation.send().await?;
            #decode
        }
    })
}

fn render_parameter_helpers(operation: &Operation, typed: &TypedOperation) -> TokenStream {
    let request_type = format_ident!("{}", typed.request_type);
    let helpers = typed.fields.chunks(32).enumerate().map(|(index, fields)| {
        let number = index.saturating_add(1);
        let helper = format_ident!("{}_parameters_{number}", operation.rust_method);
        let parameters = fields.iter().map(render_parameter);
        quote! {
            fn #helper<'client>(
                operation: crate::OperationRequest<'client>,
                request: &#request_type,
            ) -> crate::Result<crate::OperationRequest<'client>> {
                #(#parameters)*
                Ok(operation)
            }
        }
    });

    quote!(#(#helpers)*)
}

fn render_parameter(field: &crate::schema::RequestField) -> TokenStream {
    let method = format_ident!(
        "{}",
        match (field.location, field.optional) {
            (ParameterLocation::Path, false) => "path_serialized",
            (ParameterLocation::Path, true) => "path_optional_serialized",
            (ParameterLocation::Query, false) => "query_serialized",
            (ParameterLocation::Query, true) => "query_optional_serialized",
            (ParameterLocation::Header, false) => "header_serialized",
            (ParameterLocation::Header, true) => "header_optional_serialized",
        }
    );
    let field_name = format_ident!("{}", field.rust_name);
    let wire_name = &field.wire_name;
    let value = if field.optional {
        quote!(request.#field_name.as_ref())
    } else {
        quote!(&request.#field_name)
    };

    if field.location == ParameterLocation::Path {
        quote! {
            let operation = operation.#method(#wire_name, #value)?;
        }
    } else {
        let explode = field.explode;
        quote! {
            let operation = operation.#method(#wire_name, #value, #explode)?;
        }
    }
}

fn render_body(body: &crate::schema::RequestBody) -> TokenStream {
    match body.kind {
        BodyKind::Json if body.optional => quote! {
            let operation = if let Some(body) = &request.body {
                operation.json(body)?
            } else {
                operation
            };
        },
        BodyKind::Json => quote! {
            let operation = operation.json(&request.body)?;
        },
        BodyKind::Multipart => render_multipart(body),
    }
}

fn render_multipart(body: &crate::schema::RequestBody) -> TokenStream {
    let fields = body.fields.iter().map(|field| {
        let field_name = format_ident!("{}", field.rust_name);
        let wire_name = &field.wire_name;
        match (field.binary, field.optional) {
            (true, true) => quote! {
                if let Some(value) = body.#field_name {
                    form = form.part(#wire_name, value.into_part()?);
                }
            },
            (false, true) => quote! {
                if let Some(value) = body.#field_name {
                    form = form.text(#wire_name, crate::operation::multipart_text(&value)?);
                }
            },
            (true, false) => quote! {
                form = form.part(#wire_name, body.#field_name.into_part()?);
            },
            (false, false) => quote! {
                form = form.text(
                    #wire_name,
                    crate::operation::multipart_text(&body.#field_name)?,
                );
            },
        }
    });

    if body.optional {
        quote! {
            let operation = if let Some(body) = request.body {
                let mut form = reqwest::multipart::Form::new();
                #(#fields)*
                operation.multipart(form)
            } else {
                operation
            };
        }
    } else {
        quote! {
            let body = request.body;
            let mut form = reqwest::multipart::Form::new();
            #(#fields)*
            let operation = operation.multipart(form);
        }
    }
}

fn render_decode(typed: &TypedOperation) -> Result<TokenStream> {
    let body = if let Some(body_type) = &typed.response_body_type {
        let body_type = format_ident!("{body_type}");
        let arms = typed.responses.iter().map(|response| {
            let status = Literal::u16_unsuffixed(response.status);
            let variant = status_variant(response.status);
            match response.format {
                ResponseFormat::Empty => quote!(#status => #body_type::#variant,),
                ResponseFormat::Json => {
                    quote!(#status => #body_type::#variant(response.json()?),)
                }
                ResponseFormat::Text => {
                    quote!(#status => #body_type::#variant(response.text()?),)
                }
            }
        });
        quote! {
            let body = match status.as_u16() {
                #(#arms)*
                unexpected => {
                    return Err(crate::Error::InvalidResponse(format!(
                        "unexpected successful status {unexpected}"
                    )));
                }
            };
        }
    } else {
        let response = typed.responses.first().ok_or_else(|| {
            Box::new(CodegenError::Specification(String::from(
                "typed operation has no successful response",
            )))
        })?;
        let status = Literal::u16_unsuffixed(response.status);
        let body = match response.format {
            ResponseFormat::Empty => quote!(()),
            ResponseFormat::Json => quote!(response.json()?),
            ResponseFormat::Text => quote!(response.text()?),
        };
        quote! {
            if status.as_u16() != #status {
                return Err(crate::Error::InvalidResponse(format!(
                    "unexpected successful status {}",
                    status.as_u16()
                )));
            }
            let body = #body;
        }
    };

    Ok(quote! {
        let status = response.status();
        let headers = response.headers().clone();
        #body
        Ok(crate::Response::new(status, headers, body))
    })
}

fn status_variant(status: u16) -> proc_macro2::Ident {
    format_ident!(
        "{}",
        match status {
            200 => "Ok",
            201 => "Created",
            202 => "Accepted",
            204 => "NoContent",
            _ => "Success",
        }
    )
}

fn typed_operation<'operation>(
    operation: &Operation,
    typed_operations: &'operation BTreeMap<String, TypedOperation>,
) -> Result<&'operation TypedOperation> {
    typed_operations.get(&operation.id).ok_or_else(|| {
        Box::new(CodegenError::Specification(format!(
            "missing typed operation for `{}`",
            operation.id
        )))
    })
}

fn render_domain(
    domain: &str,
    operations: &[&Operation],
    typed_operations: &BTreeMap<String, TypedOperation>,
) -> Result<String> {
    let endpoints = operations
        .iter()
        .map(|operation| render_endpoint(operation))
        .collect::<Result<Vec<_>>>()?;
    let endpoint_names = operations
        .iter()
        .map(|operation| format_ident!("{}", operation.constant));
    let typed = operations
        .iter()
        .map(|operation| typed_operation(operation, typed_operations))
        .collect::<Result<Vec<_>>>()?;
    let body_aliases = typed
        .iter()
        .filter_map(|operation| render_body_alias(operation));
    let requests = typed.iter().map(|operation| render_request(operation));
    let responses = typed.iter().map(|operation| render_response(operation));
    let methods = operations
        .iter()
        .zip(&typed)
        .map(|(operation, typed)| render_method(operation, typed))
        .collect::<Result<Vec<_>>>()?;
    let parameter_helpers = operations
        .iter()
        .zip(&typed)
        .map(|(operation, typed)| render_parameter_helpers(operation, typed));
    let service = format_ident!("{}", service_type(domain)?);
    let accessor = format_ident!("{}", accessor_name(domain));

    let tokens = quote! {
        #(#endpoints)*

        /// Every supported endpoint in this domain.
        pub const ENDPOINTS: &[crate::Endpoint] = &[#(#endpoint_names),*];

        #(#body_aliases)*
        #(#requests)*
        #(#responses)*
        #(#parameter_helpers)*

        /// Operations in this Roblox Open Cloud domain.
        #[derive(Debug, Clone, Copy)]
        pub struct #service<'client> {
            client: &'client crate::Client,
        }

        impl crate::Client {
            /// Returns operations in this Roblox Open Cloud domain.
            #[must_use]
            pub const fn #accessor(&self) -> #service<'_> {
                #service { client: self }
            }
        }

        impl #service<'_> {
            #(#methods)*
        }
    };
    let rendered = crate::rust::render(tokens)?;
    Ok(format!(
        "// @generated by codegen; do not edit by hand.\n\n{rendered}"
    ))
}

fn render_coverage(operations: &[Operation]) -> Result<String> {
    let endpoints = operations.iter().map(|operation| {
        let domain = format_ident!("{}", operation.domain);
        let constant = format_ident!("{}", operation.constant);
        quote!(crate::#domain::#constant)
    });
    let tokens = quote! {
        /// Every recommended resource operation covered by this crate.
        pub const ENDPOINTS: &[crate::Endpoint] = &[#(#endpoints),*];
    };
    let rendered = crate::rust::render(tokens)?;
    Ok(format!(
        "// @generated by codegen; do not edit by hand.\n\n{rendered}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{Generated, Source, generate, route_identifier, rust_identifier};
    use crate::Result;

    const FIXTURE: &[u8] = br#"{
        "paths": {
            "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries": {
                "get": {
                    "summary": "List Data Store Entries",
                    "tags": ["Data and memory stores"],
                    "security": [{"roblox-api-key": []}],
                    "x-roblox-stability": "STABLE",
                    "parameters": [
                        {
                            "name": "universe_id",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"}
                        },
                        {
                            "name": "data_store_id",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"}
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "OK",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "entries": {
                                                "type": "array",
                                                "items": {"type": "string"}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "x-roblox-scopes": [
                        {"name": "universe-datastores.objects:list"},
                        {"name": "universe-datastores.objects:list"}
                    ]
                }
            },
            "/assets/v1/users/{userId}/quotas": {
                "get": {
                    "summary": "List Asset Quotas",
                    "tags": ["Assets"],
                    "security": [{"roblox-oauth2": []}],
                    "x-roblox-stability": "BETA",
                    "parameters": [
                        {
                            "name": "userId",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "integer", "format": "int64"}
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "OK",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "remaining": {"type": "integer", "format": "int64"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/experimental/v1/resources": {
                "post": {
                    "summary": "Experimental operation",
                    "tags": ["Assets"],
                    "security": [{"roblox-api-key": []}],
                    "x-roblox-stability": "EXPERIMENTAL"
                }
            },
            "/cookie/v1/resources": {
                "get": {
                    "summary": "Cookie operation",
                    "tags": ["Assets"],
                    "security": [{"roblox-cookie": []}],
                    "x-roblox-stability": "STABLE"
                }
            }
        },
        "components": {
            "schemas": {}
        }
    }"#;

    fn source() -> Source {
        Source::creator_docs(String::from("fixture-commit"), String::from("2026-07-31"))
    }

    fn fixture() -> Result<Generated> {
        generate(FIXTURE, &source())
    }

    #[test]
    fn identifiers_are_stable_and_valid() {
        assert_eq!(
            rust_identifier("List Asset Quotas"),
            "list_asset_quotas",
            "summaries should become snake-case Rust identifiers"
        );
        assert_eq!(
            rust_identifier("type"),
            "type_value",
            "Rust keywords should receive a stable suffix"
        );
        assert_eq!(
            rust_identifier("123 Places"),
            "operation_123_places",
            "identifiers may not begin with a digit"
        );
        assert_eq!(
            route_identifier(
                "GET",
                "/cloud/v2/universes/{universe_id}/data-stores/{data_store_id}/entries"
            ),
            "get_data_stores_data_store_id_entries",
            "route identifiers should use the final meaningful path segments"
        );
    }

    #[test]
    fn generation_filters_and_types_every_domain_uniformly() -> Result<()> {
        let generated = fixture()?;
        let datastore = generated
            .domains()
            .get("datastore")
            .map_or("", String::as_str);
        let assets = generated.domains().get("assets").map_or("", String::as_str);

        assert!(
            generated.coverage_json().contains("\"count\": 2"),
            "only recommended API-key, OAuth, or stable unauthenticated operations should remain"
        );
        assert!(
            datastore.contains("pub struct ListDataStoreEntriesRequest"),
            "every operation should receive a typed request"
        );
        assert!(
            datastore.contains("pub async fn list_data_store_entries("),
            "formerly handwritten domains should use the generated typed method"
        );
        assert!(
            datastore.contains("impl ListDataStoreEntriesRequest")
                && datastore.contains("data_store_id: impl Into<String>"),
            "every request should receive the same generated constructor shape"
        );
        assert!(
            assets.contains("pub struct ListAssetQuotasRequest"),
            "ordinary domains should receive the same typed request shape"
        );
        assert!(
            assets.contains("pub async fn list_asset_quotas("),
            "ordinary domains should use the same generated typed method"
        );
        assert!(
            !generated.coverage_json().contains("Experimental operation"),
            "experimental operations should be excluded"
        );
        assert!(
            !generated.coverage_json().contains("Cookie operation"),
            "cookie-only operations should be excluded"
        );
        Ok(())
    }

    #[test]
    fn generation_is_deterministic() -> Result<()> {
        let first = fixture()?;
        let second = fixture()?;
        assert_eq!(
            first.coverage_json(),
            second.coverage_json(),
            "coverage generation should be deterministic"
        );
        assert_eq!(
            first.domains(),
            second.domains(),
            "Rust source generation should be deterministic"
        );
        assert_eq!(
            first.types(),
            second.types(),
            "model generation should be deterministic"
        );
        Ok(())
    }
}
