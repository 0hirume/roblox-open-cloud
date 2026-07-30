use std::collections::{BTreeMap, BTreeSet};

use heck::ToUpperCamelCase;
use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use serde_json::{Map, Value};
use syn::{Type, parse_quote};

use crate::generate::rust_identifier;
use crate::{CodegenError, Result};

const JSON_MEDIA_TYPES: &[&str] = &[
    "application/json",
    "text/json",
    "application/json-patch+json",
    "application/*+json",
];

#[derive(Debug, Clone)]
pub(super) struct OperationInput {
    pub(super) id: String,
    pub(super) domain: String,
    pub(super) method: String,
    pub(super) path: String,
    pub(super) rust_method: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParameterLocation {
    Path,
    Query,
    Header,
}

#[derive(Debug, Clone)]
pub(super) struct RequestField {
    pub(super) rust_name: String,
    pub(super) wire_name: String,
    pub(super) rust_type: Type,
    pub(super) optional: bool,
    pub(super) location: ParameterLocation,
    pub(super) explode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BodyKind {
    Json,
    Multipart,
}

#[derive(Debug, Clone)]
pub(super) struct MultipartField {
    pub(super) rust_name: String,
    pub(super) wire_name: String,
    pub(super) binary: bool,
    pub(super) optional: bool,
}

#[derive(Debug, Clone)]
pub(super) struct RequestBody {
    pub(super) rust_type: Type,
    pub(super) optional: bool,
    pub(super) kind: BodyKind,
    pub(super) fields: Vec<MultipartField>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResponseFormat {
    Empty,
    Json,
    Text,
}

#[derive(Debug, Clone)]
pub(super) struct SuccessResponse {
    pub(super) status: u16,
    pub(super) rust_type: Type,
    pub(super) format: ResponseFormat,
}

#[derive(Debug, Clone)]
pub(super) struct TypedOperation {
    pub(super) request_type: String,
    pub(super) response_type: String,
    pub(super) body_alias: Option<String>,
    pub(super) response_body_type: Option<String>,
    pub(super) fields: Vec<RequestField>,
    pub(super) body: Option<RequestBody>,
    pub(super) responses: Vec<SuccessResponse>,
}

#[derive(Debug)]
pub(super) struct Generated {
    pub(super) types: String,
    pub(super) operations: BTreeMap<String, TypedOperation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SerdeMode {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone)]
struct Definition {
    schema: Value,
    serde: SerdeMode,
    constructible: bool,
}

#[derive(Debug)]
struct StructField {
    rust_name: String,
    wire_name: String,
    rust_type: Type,
    optional: bool,
    flatten: bool,
}

#[derive(Debug)]
struct Registry {
    component_schemas: BTreeMap<String, Value>,
    component_names: BTreeMap<String, String>,
    definitions: BTreeMap<String, Definition>,
    used_names: BTreeSet<String>,
}

impl Registry {
    fn new(api: &Value) -> Result<Self> {
        let schemas = object_at(api, &["components", "schemas"])?;
        let mut component_names = BTreeMap::new();
        let mut used_names = BTreeSet::new();
        for component_name in schemas.keys() {
            let preferred = type_identifier(component_name);
            let rust_name = unique_name(&preferred, &mut used_names);
            let previous = component_names.insert(component_name.clone(), rust_name);
            if previous.is_some() {
                return specification("an OpenAPI component was registered more than once");
            }
        }

        Ok(Self {
            component_schemas: schemas
                .iter()
                .map(|(name, schema)| (name.clone(), schema.clone()))
                .collect(),
            component_names,
            definitions: BTreeMap::new(),
            used_names,
        })
    }

    fn type_for(
        &mut self,
        preferred: &str,
        schema: &Value,
        serde: SerdeMode,
        constructible: bool,
    ) -> Result<Type> {
        let schema = normalize_schema(schema);
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            return self.reference_type(reference, constructible);
        }
        if is_alias_all_of(schema) {
            let item = schema
                .get("allOf")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .ok_or_else(|| {
                    Box::new(CodegenError::Specification(String::from(
                        "an allOf alias did not contain a schema",
                    )))
                })?;
            return self.type_for(preferred, item, serde, constructible);
        }

        if schema.get("enum").and_then(Value::as_array).is_some() {
            return self.register_inline(preferred, schema, SerdeMode::Enabled, constructible);
        }

        match schema.get("type").and_then(Value::as_str) {
            Some("array") => {
                let items = schema.get("items").ok_or_else(|| {
                    Box::new(CodegenError::Specification(format!(
                        "array schema `{preferred}` has no items"
                    )))
                })?;
                let item_type = self.type_for(
                    &format!("{preferred}Item"),
                    items,
                    SerdeMode::Enabled,
                    constructible,
                )?;
                Ok(parse_quote!(Vec<#item_type>))
            }
            Some("boolean") => Ok(parse_quote!(bool)),
            Some("integer") => Ok(integer_type(schema)),
            Some("number") => Ok(number_type(schema)),
            Some("string")
                if schema.get("format").and_then(Value::as_str) == Some("binary")
                    && serde == SerdeMode::Disabled =>
            {
                Ok(parse_quote!(crate::File))
            }
            Some("string") => Ok(parse_quote!(String)),
            Some("object") if is_named_object(schema) => {
                self.register_inline(preferred, schema, serde, constructible)
            }
            Some("object") => self.map_type(preferred, schema, constructible),
            Some(other) => specification(format!(
                "schema `{preferred}` uses unsupported type `{other}`"
            )),
            None if schema.get("properties").is_some() || schema.get("allOf").is_some() => {
                self.register_inline(preferred, schema, serde, constructible)
            }
            None => Ok(parse_quote!(serde_json::Value)),
        }
    }

    fn map_type(&mut self, preferred: &str, schema: &Value, constructible: bool) -> Result<Type> {
        match schema.get("additionalProperties") {
            Some(Value::Object(_)) => {
                let value_type = self.type_for(
                    &format!("{preferred}Value"),
                    schema.get("additionalProperties").ok_or_else(|| {
                        Box::new(CodegenError::Specification(format!(
                            "map schema `{preferred}` lost its value schema"
                        )))
                    })?,
                    SerdeMode::Enabled,
                    constructible,
                )?;
                Ok(parse_quote!(
                    std::collections::BTreeMap<String, #value_type>
                ))
            }
            Some(Value::Bool(true)) => Ok(parse_quote!(
                std::collections::BTreeMap<String, serde_json::Value>
            )),
            _ => Ok(parse_quote!(serde_json::Value)),
        }
    }

    fn reference_type(&mut self, reference: &str, constructible: bool) -> Result<Type> {
        let component_name = reference
            .strip_prefix("#/components/schemas/")
            .ok_or_else(|| {
                Box::new(CodegenError::Specification(format!(
                    "unsupported schema reference `{reference}`"
                )))
            })?;
        let rust_name = self
            .component_names
            .get(component_name)
            .ok_or_else(|| {
                Box::new(CodegenError::Specification(format!(
                    "schema reference `{reference}` does not exist"
                )))
            })?
            .clone();
        if let Some(definition) = self.definitions.get_mut(&rust_name) {
            definition.constructible |= constructible;
        } else {
            let schema = self
                .component_schemas
                .get(component_name)
                .ok_or_else(|| {
                    Box::new(CodegenError::Specification(format!(
                        "schema reference `{reference}` has no definition"
                    )))
                })?
                .clone();
            let previous = self.definitions.insert(
                rust_name.clone(),
                Definition {
                    schema,
                    serde: SerdeMode::Enabled,
                    constructible,
                },
            );
            if previous.is_some() {
                return specification(format!(
                    "schema reference `{reference}` was registered more than once"
                ));
            }
        }
        let rust_name = format_ident!("{rust_name}");
        Ok(parse_quote!(crate::types::#rust_name))
    }

    fn register_inline(
        &mut self,
        preferred: &str,
        schema: &Value,
        serde: SerdeMode,
        constructible: bool,
    ) -> Result<Type> {
        let base_name = type_identifier(preferred);
        if let Some(existing) = self.definitions.get_mut(&base_name) {
            if existing.schema == *schema && existing.serde == serde {
                existing.constructible |= constructible;
                let base_name = format_ident!("{base_name}");
                return Ok(parse_quote!(crate::types::#base_name));
            }
        }
        let rust_name = unique_name(&base_name, &mut self.used_names);
        let previous = self.definitions.insert(
            rust_name.clone(),
            Definition {
                schema: schema.clone(),
                serde,
                constructible,
            },
        );
        if previous.is_some() {
            return specification(format!(
                "inline schema `{preferred}` overwrote `{rust_name}`"
            ));
        }
        let rust_name = format_ident!("{rust_name}");
        Ok(parse_quote!(crate::types::#rust_name))
    }

    fn render(mut self) -> Result<String> {
        let mut rendered = BTreeSet::new();
        let mut sources = Vec::new();
        loop {
            let next = self
                .definitions
                .keys()
                .find(|name| !rendered.contains(*name))
                .cloned();
            let Some(name) = next else {
                break;
            };
            let definition = self.definitions.get(&name).cloned().ok_or_else(|| {
                Box::new(CodegenError::Specification(format!(
                    "type definition `{name}` disappeared during rendering"
                )))
            })?;
            sources.push(self.render_definition(&name, &definition)?);
            let inserted = rendered.insert(name);
            if !inserted {
                return specification("a generated type was rendered more than once");
            }
        }

        let source = crate::rust::render(quote!(#(#sources)*))?;
        Ok(format!(
            "// @generated by codegen; do not edit by hand.\n\n{source}"
        ))
    }

    fn render_definition(&mut self, name: &str, definition: &Definition) -> Result<TokenStream> {
        let schema = normalize_schema(&definition.schema).clone();
        if schema.get("enum").and_then(Value::as_array).is_some() {
            return render_enum(name, &schema, definition.serde);
        }
        if is_alias_all_of(&schema) {
            let item = schema
                .get("allOf")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .ok_or_else(|| {
                    Box::new(CodegenError::Specification(format!(
                        "alias `{name}` has no target"
                    )))
                })?;
            let target = self.type_for(
                &format!("{name}Value"),
                item,
                definition.serde,
                definition.constructible,
            )?;
            return Ok(render_alias(name, &target));
        }

        match schema.get("type").and_then(Value::as_str) {
            Some("string") => Ok(render_alias(name, &parse_quote!(String))),
            Some("integer") => Ok(render_alias(name, &integer_type(&schema))),
            Some("number") => Ok(render_alias(name, &number_type(&schema))),
            Some("boolean") => Ok(render_alias(name, &parse_quote!(bool))),
            Some("array") => {
                let target = self.type_for(
                    &format!("{name}Item"),
                    schema.get("items").ok_or_else(|| {
                        Box::new(CodegenError::Specification(format!(
                            "array type `{name}` has no items"
                        )))
                    })?,
                    SerdeMode::Enabled,
                    definition.constructible,
                )?;
                Ok(render_alias(name, &parse_quote!(Vec<#target>)))
            }
            Some("object") | None => {
                self.render_object(name, &schema, definition.serde, definition.constructible)
            }
            Some(other) => specification(format!(
                "type definition `{name}` uses unsupported type `{other}`"
            )),
        }
    }

    fn render_object(
        &mut self,
        name: &str,
        schema: &Value,
        serde: SerdeMode,
        constructible: bool,
    ) -> Result<TokenStream> {
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let all_of = schema
            .get("allOf")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if properties.is_empty() && all_of.is_empty() {
            if matches!(schema.get("additionalProperties"), Some(Value::Object(_)))
                || !matches!(schema.get("additionalProperties"), Some(Value::Bool(false)))
            {
                let target = self.map_type(name, schema, constructible)?;
                return Ok(render_alias(name, &target));
            }
            return Ok(render_empty_object(name, serde, constructible));
        }

        let required = required_fields(schema);
        let defaultable = all_of.is_empty()
            && properties.iter().all(|(wire_name, property)| {
                !required.contains(wire_name.as_str()) || is_nullable(property)
            });
        let mut field_names = BTreeSet::new();
        let mut fields = Vec::new();

        for (index, base_schema) in all_of.iter().enumerate() {
            let number = index.saturating_add(1);
            let base_type = self.type_for(
                &format!("{name}Base{number}"),
                base_schema,
                serde,
                constructible,
            )?;
            let base_name = unique_field_name(
                if index == 0 {
                    String::from("base")
                } else {
                    format!("base_{number}")
                },
                &mut field_names,
            );
            fields.push(StructField {
                rust_name: base_name.clone(),
                wire_name: base_name,
                rust_type: base_type,
                optional: false,
                flatten: true,
            });
        }

        for (wire_name, property_schema) in properties {
            let rust_name = unique_field_name(rust_identifier(&wire_name), &mut field_names);
            let property_serde =
                if property_schema.get("format").and_then(Value::as_str) == Some("binary") {
                    serde
                } else {
                    SerdeMode::Enabled
                };
            let field_type = self.type_for(
                &format!("{name}{}", type_identifier(&wire_name)),
                &property_schema,
                property_serde,
                constructible,
            )?;
            let optional = !required.contains(wire_name.as_str()) || is_nullable(&property_schema);
            fields.push(StructField {
                rust_name,
                wire_name,
                rust_type: field_type,
                optional,
                flatten: false,
            });
        }

        if matches!(
            schema.get("additionalProperties"),
            Some(Value::Bool(true) | Value::Object(_))
        ) {
            let rust_name =
                unique_field_name(String::from("additional_properties"), &mut field_names);
            let map_type = self.map_type(&format!("{name}Additional"), schema, constructible)?;
            fields.push(StructField {
                rust_name: rust_name.clone(),
                wire_name: rust_name,
                rust_type: map_type,
                optional: false,
                flatten: true,
            });
        }

        Ok(render_struct(
            name,
            &fields,
            serde,
            defaultable,
            constructible,
        ))
    }
}

pub(super) fn generate(api: &Value, inputs: &[OperationInput]) -> Result<Generated> {
    let mut registry = Registry::new(api)?;
    let mut operations = BTreeMap::new();
    for input in inputs {
        let operation = prepare_operation(api, input, &mut registry)?;
        let previous = operations.insert(input.id.clone(), operation);
        if previous.is_some() {
            return specification(format!(
                "typed operation `{}` was generated more than once",
                input.id
            ));
        }
    }
    let types = registry.render()?;
    Ok(Generated { types, operations })
}

fn prepare_operation(
    api: &Value,
    input: &OperationInput,
    registry: &mut Registry,
) -> Result<TypedOperation> {
    let path_item = object_at(api, &["paths"])?
        .get(&input.path)
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "covered path `{}` is missing",
                input.path
            )))
        })?;
    let operation = path_item
        .get(&input.method.to_ascii_lowercase())
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "covered operation `{} {}` is missing",
                input.method, input.path
            )))
        })?;
    let type_prefix = format!(
        "{}{}",
        type_identifier(&input.domain),
        type_identifier(&input.rust_method)
    );
    let fields = prepare_parameters(path_item, operation, &type_prefix, registry)?;
    let body = prepare_body(api, operation, &type_prefix, registry)?;
    let body_alias = body
        .as_ref()
        .map(|_| format!("{}Body", type_identifier(&input.rust_method)));
    let responses = prepare_responses(operation, &type_prefix, registry)?;
    let response_body_type = (responses.len() > 1).then(|| format!("{type_prefix}ResponseBody"));

    Ok(TypedOperation {
        request_type: format!("{}Request", type_identifier(&input.rust_method)),
        response_type: format!("{}Response", type_identifier(&input.rust_method)),
        body_alias,
        response_body_type,
        fields,
        body,
        responses,
    })
}

fn prepare_parameters(
    path_item: &Map<String, Value>,
    operation: &Map<String, Value>,
    type_prefix: &str,
    registry: &mut Registry,
) -> Result<Vec<RequestField>> {
    let mut parameters = BTreeMap::<(String, String), Value>::new();
    for source in [path_item.get("parameters"), operation.get("parameters")] {
        for parameter in source.and_then(Value::as_array).into_iter().flatten() {
            let object = parameter.as_object().ok_or_else(|| {
                Box::new(CodegenError::Specification(String::from(
                    "an operation parameter is not an object",
                )))
            })?;
            let wire_name = required_str(object, "name")?;
            let location = required_str(object, "in")?;
            let _previous = parameters.insert(
                (String::from(location), String::from(wire_name)),
                parameter.clone(),
            );
        }
    }

    let mut used_names = BTreeSet::new();
    let mut fields = Vec::with_capacity(parameters.len());
    for ((_location, _wire_name), parameter) in parameters {
        let object = parameter.as_object().ok_or_else(|| {
            Box::new(CodegenError::Specification(String::from(
                "an operation parameter is not an object",
            )))
        })?;
        let wire_name = required_str(object, "name")?;
        let location = match required_str(object, "in")? {
            "path" => ParameterLocation::Path,
            "query" => ParameterLocation::Query,
            "header" => ParameterLocation::Header,
            other => {
                return specification(format!(
                    "parameter `{wire_name}` uses unsupported location `{other}`"
                ));
            }
        };
        let rust_name = unique_field_name(rust_identifier(wire_name), &mut used_names);
        let schema = object.get("schema").ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "parameter `{wire_name}` has no schema"
            )))
        })?;
        let rust_type = registry.type_for(
            &format!("{type_prefix}{}", type_identifier(wire_name)),
            schema,
            SerdeMode::Enabled,
            false,
        )?;
        let optional = !object
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || is_nullable(schema);
        let explode = object
            .get("explode")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        fields.push(RequestField {
            rust_name,
            wire_name: String::from(wire_name),
            rust_type,
            optional,
            location,
            explode,
        });
    }
    fields.sort_by(|left, right| {
        location_rank(left.location)
            .cmp(&location_rank(right.location))
            .then_with(|| left.wire_name.cmp(&right.wire_name))
    });
    Ok(fields)
}

fn prepare_body(
    api: &Value,
    operation: &Map<String, Value>,
    type_prefix: &str,
    registry: &mut Registry,
) -> Result<Option<RequestBody>> {
    let Some(raw_body) = operation.get("requestBody") else {
        return Ok(None);
    };
    let body = resolve_request_body(api, raw_body)?;
    let required = body
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let content = body
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "request body `{type_prefix}` has no content"
            )))
        })?;

    if let Some(media) = content.get("multipart/form-data") {
        let schema = media.get("schema").ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "multipart body `{type_prefix}` has no schema"
            )))
        })?;
        let rust_type = registry.register_inline(
            &format!("{type_prefix}Body"),
            schema,
            SerdeMode::Disabled,
            true,
        )?;
        let fields = multipart_fields(schema)?;
        return Ok(Some(RequestBody {
            rust_type,
            optional: !required,
            kind: BodyKind::Multipart,
            fields,
        }));
    }

    for media_type in JSON_MEDIA_TYPES {
        if let Some(media) = content.get(*media_type) {
            let schema = media.get("schema").ok_or_else(|| {
                Box::new(CodegenError::Specification(format!(
                    "JSON body `{type_prefix}` has no schema"
                )))
            })?;
            let rust_type = registry.type_for(
                &format!("{type_prefix}Body"),
                schema,
                SerdeMode::Enabled,
                true,
            )?;
            return Ok(Some(RequestBody {
                rust_type,
                optional: !required,
                kind: BodyKind::Json,
                fields: Vec::new(),
            }));
        }
    }

    specification(format!(
        "request body `{type_prefix}` has no supported media type"
    ))
}

fn prepare_responses(
    operation: &Map<String, Value>,
    type_prefix: &str,
    registry: &mut Registry,
) -> Result<Vec<SuccessResponse>> {
    let responses = operation
        .get("responses")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "operation `{type_prefix}` has no responses"
            )))
        })?;
    let mut success = Vec::new();
    for (status, response) in responses {
        let Ok(status) = status.parse::<u16>() else {
            continue;
        };
        if !(200..300).contains(&status) {
            continue;
        }
        let response = response.as_object().ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "response `{type_prefix} {status}` is not an object"
            )))
        })?;
        let Some(content) = response.get("content").and_then(Value::as_object) else {
            success.push(SuccessResponse {
                status,
                rust_type: parse_quote!(()),
                format: ResponseFormat::Empty,
            });
            continue;
        };
        if content.is_empty() {
            success.push(SuccessResponse {
                status,
                rust_type: parse_quote!(()),
                format: ResponseFormat::Empty,
            });
            continue;
        }
        if let Some(media) = content
            .get("application/json")
            .or_else(|| content.get("text/json"))
        {
            let schema = media.get("schema").ok_or_else(|| {
                Box::new(CodegenError::Specification(format!(
                    "JSON response `{type_prefix} {status}` has no schema"
                )))
            })?;
            let rust_type = registry.type_for(
                &format!("{type_prefix}Status{status}"),
                schema,
                SerdeMode::Enabled,
                false,
            )?;
            success.push(SuccessResponse {
                status,
                rust_type,
                format: ResponseFormat::Json,
            });
        } else if content.contains_key("text/plain") {
            success.push(SuccessResponse {
                status,
                rust_type: parse_quote!(String),
                format: ResponseFormat::Text,
            });
        } else {
            return specification(format!(
                "response `{type_prefix} {status}` has no supported media type"
            ));
        }
    }
    success.sort_by_key(|response| response.status);
    if success.is_empty() {
        specification(format!(
            "operation `{type_prefix}` has no successful response"
        ))
    } else {
        Ok(success)
    }
}

fn multipart_fields(schema: &Value) -> Result<Vec<MultipartField>> {
    let schema = normalize_schema(schema);
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Box::new(CodegenError::Specification(String::from(
                "multipart request schema has no properties",
            )))
        })?;
    let required: BTreeSet<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut used_names = BTreeSet::new();
    let mut fields = Vec::with_capacity(properties.len());
    for (wire_name, property) in properties {
        fields.push(MultipartField {
            rust_name: unique_field_name(rust_identifier(wire_name), &mut used_names),
            wire_name: wire_name.clone(),
            binary: property.get("format").and_then(Value::as_str) == Some("binary"),
            optional: !required.contains(wire_name.as_str()) || is_nullable(property),
        });
    }
    Ok(fields)
}

fn resolve_request_body<'api>(
    api: &'api Value,
    body: &'api Value,
) -> Result<&'api Map<String, Value>> {
    if let Some(reference) = body.get("$ref").and_then(Value::as_str) {
        let name = reference
            .strip_prefix("#/components/requestBodies/")
            .ok_or_else(|| {
                Box::new(CodegenError::Specification(format!(
                    "unsupported request body reference `{reference}`"
                )))
            })?;
        return object_at(api, &["components", "requestBodies"])?
            .get(name)
            .and_then(Value::as_object)
            .ok_or_else(|| {
                Box::new(CodegenError::Specification(format!(
                    "request body reference `{reference}` does not exist"
                )))
            });
    }
    body.as_object().ok_or_else(|| {
        Box::new(CodegenError::Specification(String::from(
            "request body is not an object",
        )))
    })
}

fn render_enum(name: &str, schema: &Value, serde: SerdeMode) -> Result<TokenStream> {
    let values = schema
        .get("enum")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "enum `{name}` has no values"
            )))
        })?;
    if values.iter().all(Value::is_string) {
        return render_string_enum(name, schema, values, serde);
    }
    if values.iter().all(Value::is_i64) {
        return render_integer_enum(name, schema, values, serde);
    }
    specification(format!(
        "enum `{name}` mixes unsupported value representations"
    ))
}

fn render_string_enum(
    name: &str,
    schema: &Value,
    values: &[Value],
    serde: SerdeMode,
) -> Result<TokenStream> {
    let provided_names = enum_names(schema);
    let mut used_names = BTreeSet::new();
    let mut variants = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let wire_value = value.as_str().ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "string enum `{name}` contains a non-string"
            )))
        })?;
        let preferred = provided_names.get(index).map_or(wire_value, String::as_str);
        let variant = unique_name(&type_identifier(preferred), &mut used_names);
        let variant = format_ident!("{variant}");
        let rename = if serde == SerdeMode::Enabled {
            quote!(#[serde(rename = #wire_value)])
        } else {
            TokenStream::new()
        };
        variants.push(quote!(
            #rename
            #variant,
        ));
    }
    let derives = if serde == SerdeMode::Enabled {
        quote!(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            serde::Serialize,
            serde::Deserialize
        )
    } else {
        quote!(Debug, Clone, Copy, PartialEq, Eq, Hash)
    };
    let doc = format!("OpenAPI enum `{name}`.");
    let name = format_ident!("{name}");
    Ok(quote! {
        #[doc = #doc]
        #[derive(#derives)]
        pub enum #name {
            #(#variants)*
        }
    })
}

fn render_integer_enum(
    name: &str,
    schema: &Value,
    values: &[Value],
    serde: SerdeMode,
) -> Result<TokenStream> {
    let provided_names = enum_names(schema);
    let integer = integer_type(schema);
    let mut used_names = BTreeSet::new();
    let mut constants = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let number = value.as_i64().ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "integer enum `{name}` contains a non-integer"
            )))
        })?;
        let fallback = if number < 0 {
            format!("negative_{}", number.unsigned_abs())
        } else {
            format!("value_{number}")
        };
        let preferred = provided_names
            .get(index)
            .map_or(fallback.as_str(), String::as_str);
        let constant =
            unique_field_name(rust_identifier(preferred).to_uppercase(), &mut used_names);
        let constant = format_ident!("{constant}");
        let number = Literal::i64_unsuffixed(number);
        constants.push(quote!(pub const #constant: Self = Self(#number);));
    }
    let serde_attribute = if serde == SerdeMode::Enabled {
        quote!(#[serde(transparent)])
    } else {
        TokenStream::new()
    };
    let derives = if serde == SerdeMode::Enabled {
        quote!(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            serde::Serialize,
            serde::Deserialize
        )
    } else {
        quote!(Debug, Clone, Copy, PartialEq, Eq, Hash)
    };
    let doc = format!("OpenAPI integer enum `{name}`.");
    let name = format_ident!("{name}");
    Ok(quote! {
        #[doc = #doc]
        #[derive(#derives)]
        #serde_attribute
        pub struct #name(pub #integer);

        impl #name {
            #(#constants)*
        }
    })
}

fn render_alias(name: &str, target: &Type) -> TokenStream {
    let doc = format!("OpenAPI type `{name}`.");
    let name = format_ident!("{name}");
    quote! {
        #[doc = #doc]
        pub type #name = #target;
    }
}

fn render_empty_object(name: &str, serde: SerdeMode, constructible: bool) -> TokenStream {
    let doc = format!("Empty OpenAPI object `{name}`.");
    let name = format_ident!("{name}");
    let implementation = if constructible {
        let value = if serde == SerdeMode::Enabled {
            quote!(Self { empty: () })
        } else {
            quote!(Self)
        };
        quote! {
            impl #name {
                #[doc = "Creates an empty request object."]
                #[must_use]
                pub const fn new() -> Self {
                    #value
                }
            }
        }
    } else {
        TokenStream::new()
    };
    if serde == SerdeMode::Enabled {
        quote! {
            #[doc = #doc]
            #[derive(
                Debug,
                Clone,
                Copy,
                Default,
                PartialEq,
                Eq,
                serde::Serialize,
                serde::Deserialize
            )]
            pub struct #name {
                #[serde(skip)]
                empty: (),
            }

            #implementation
        }
    } else {
        quote! {
            #[doc = #doc]
            #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
            pub struct #name;

            #implementation
        }
    }
}

fn render_struct(
    name: &str,
    fields: &[StructField],
    serde: SerdeMode,
    defaultable: bool,
    constructible: bool,
) -> TokenStream {
    let derives = match (serde, defaultable) {
        (SerdeMode::Enabled, true) => {
            quote!(Debug, Clone, Default, serde::Serialize, serde::Deserialize)
        }
        (SerdeMode::Enabled, false) => {
            quote!(Debug, Clone, serde::Serialize, serde::Deserialize)
        }
        (SerdeMode::Disabled, true) => quote!(Debug, Clone, Default),
        (SerdeMode::Disabled, false) => quote!(Debug, Clone),
    };
    let rendered_fields = fields.iter().map(|field| render_field(field, serde));
    let implementation = if constructible {
        render_struct_impl(name, fields)
    } else {
        TokenStream::new()
    };
    let doc = format!("OpenAPI object `{name}`.");
    let name = format_ident!("{name}");
    quote! {
        #[doc = #doc]
        #[derive(#derives)]
        pub struct #name {
            #(#rendered_fields)*
        }

        #implementation
    }
}

fn render_field(field: &StructField, serde: SerdeMode) -> TokenStream {
    let doc = format!("OpenAPI field `{}`.", field.wire_name);
    let rust_name = format_ident!("{}", field.rust_name);
    let wire_name = &field.wire_name;
    let rust_type = &field.rust_type;
    let serde_attribute = if serde != SerdeMode::Enabled {
        TokenStream::new()
    } else if field.flatten {
        quote!(#[serde(flatten)])
    } else if field.optional {
        quote!(
            #[serde(
                rename = #wire_name,
                default,
                skip_serializing_if = "Option::is_none"
            )]
        )
    } else {
        quote!(#[serde(rename = #wire_name)])
    };
    let field_type = if field.optional {
        quote!(Option<#rust_type>)
    } else {
        quote!(#rust_type)
    };
    quote! {
        #[doc = #doc]
        #serde_attribute
        pub #rust_name: #field_type,
    }
}

fn render_struct_impl(name: &str, fields: &[StructField]) -> TokenStream {
    let required: Vec<&StructField> = fields.iter().filter(|field| !field.optional).collect();
    let parameters = required.iter().map(|field| {
        let rust_name = format_ident!("{}", field.rust_name);
        let rust_type = &field.rust_type;
        quote!(#rust_name: impl Into<#rust_type>)
    });
    let initializers = fields.iter().map(|field| {
        let rust_name = format_ident!("{}", field.rust_name);
        if field.optional {
            quote!(#rust_name: None,)
        } else {
            quote!(#rust_name: #rust_name.into(),)
        }
    });
    let constructor = if required.is_empty() {
        quote! {
            #[doc = "Creates this value from its required fields."]
            #[must_use]
            pub const fn new() -> Self {
                Self {
                    #(#initializers)*
                }
            }
        }
    } else {
        quote! {
            #[doc = "Creates this value from its required fields."]
            #[must_use]
            pub fn new(#(#parameters),*) -> Self {
                Self {
                    #(#initializers)*
                }
            }
        }
    };
    let setters = fields
        .iter()
        .filter(|field| field.optional)
        .map(render_struct_setter);
    let name = format_ident!("{name}");
    quote! {
        impl #name {
            #constructor
            #(#setters)*
        }
    }
}

fn render_struct_setter(field: &StructField) -> TokenStream {
    let doc = format!("Sets `{}`.", field.wire_name);
    let rust_name = format_ident!("{}", field.rust_name);
    let rust_type = &field.rust_type;
    quote! {
        #[doc = #doc]
        #[must_use]
        pub fn #rust_name(mut self, #rust_name: impl Into<#rust_type>) -> Self {
            self.#rust_name = Some(#rust_name.into());
            self
        }
    }
}

fn enum_names(schema: &Value) -> Vec<String> {
    for key in ["x-enum-varnames", "x-enumNames"] {
        if let Some(names) = schema.get(key).and_then(Value::as_array) {
            return names
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect();
        }
    }
    Vec::new()
}

fn normalize_schema(schema: &Value) -> &Value {
    schema
        .get("type")
        .filter(|value| value.is_object())
        .unwrap_or(schema)
}

fn required_fields(schema: &Value) -> BTreeSet<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn is_alias_all_of(schema: &Value) -> bool {
    schema
        .get("allOf")
        .and_then(Value::as_array)
        .is_some_and(|items| items.len() == 1)
        && schema
            .get("properties")
            .and_then(Value::as_object)
            .is_none_or(Map::is_empty)
}

fn is_named_object(schema: &Value) -> bool {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|properties| !properties.is_empty())
        || schema
            .get("allOf")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        || matches!(schema.get("additionalProperties"), Some(Value::Bool(false)))
}

fn is_nullable(schema: &Value) -> bool {
    schema
        .get("nullable")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn integer_type(schema: &Value) -> Type {
    match schema.get("format").and_then(Value::as_str) {
        Some("int32") => parse_quote!(i32),
        _ => parse_quote!(i64),
    }
}

fn number_type(schema: &Value) -> Type {
    match schema.get("format").and_then(Value::as_str) {
        Some("float") => parse_quote!(f32),
        _ => parse_quote!(f64),
    }
}

fn object_at<'value>(value: &'value Value, path: &[&str]) -> Result<&'value Map<String, Value>> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment).ok_or_else(|| {
            Box::new(CodegenError::Specification(format!(
                "OpenAPI document is missing `{}`",
                path.join(".")
            )))
        })?;
    }
    current.as_object().ok_or_else(|| {
        Box::new(CodegenError::Specification(format!(
            "OpenAPI value `{}` is not an object",
            path.join(".")
        )))
    })
}

fn required_str<'object>(object: &'object Map<String, Value>, key: &str) -> Result<&'object str> {
    object.get(key).and_then(Value::as_str).ok_or_else(|| {
        Box::new(CodegenError::Specification(format!(
            "OpenAPI object is missing string `{key}`"
        )))
    })
}

fn type_identifier(value: &str) -> String {
    let camel = value.to_upper_camel_case();
    let mut identifier: String = camel.chars().filter(char::is_ascii_alphanumeric).collect();
    if identifier
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        identifier = format!("Type{identifier}");
    } else if identifier.is_empty() {
        identifier = String::from("GeneratedType");
    } else if identifier == "Self" {
        identifier = String::from("SelfValue");
    }
    identifier
}

fn unique_name(preferred: &str, used: &mut BTreeSet<String>) -> String {
    if used.insert(String::from(preferred)) {
        return String::from(preferred);
    }
    let mut suffix = 2_u64;
    loop {
        let candidate = format!("{preferred}{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}

fn unique_field_name(preferred: String, used: &mut BTreeSet<String>) -> String {
    if used.insert(preferred.clone()) {
        return preferred;
    }
    let mut suffix = 2_u64;
    loop {
        let candidate = format!("{preferred}_{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}

const fn location_rank(location: ParameterLocation) -> u8 {
    match location {
        ParameterLocation::Path => 0,
        ParameterLocation::Query => 1,
        ParameterLocation::Header => 2,
    }
}

fn specification<T>(message: impl Into<String>) -> Result<T> {
    Err(Box::new(CodegenError::Specification(message.into())))
}
