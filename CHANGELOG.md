# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-07-30

### Changed

- Replaced the Nushell synchronization script with an unpublished, strictly
  linted and tested Rust code-generation workspace tool.
- Vendored the Creator Docs OpenAPI input so generation is reproducible without
  a separate checkout or machine-specific path.
- Added a scheduled workflow that refreshes the vendored OpenAPI snapshot and
  opens a review PR when its generated API changes.
- Added push and pull-request checks for generated output, formatting, lints,
  tests, documentation, and package verification.
- Configured releases to create dated changelog sections automatically.
- Replaced the split handwritten/request-builder resource API with one uniform,
  fully typed request-and-response API generated from the recommended Creator
  Docs OpenAPI surface.
- Generated reusable OpenAPI models for JSON and multipart operations across
  all 28 supported resource domains.
- Added generated `new(required...)` constructors, chainable optional-field
  setters, `Into<T>` conversions, and concise domain-local request-body aliases
  across the resource API.

### Removed

- Removed the old handwritten resource clients and generic domain request
  builders in favor of the uniform generated API.

## [0.1.0] - 2026-07-30

### Added

- API-key and OAuth authentication, including API-key introspection and the
  OAuth authorization-code lifecycle with PKCE.
- Typed APIs for universes, standard and ordered DataStores, MemoryStore, and
  cross-server messaging.
- Request-builder coverage for 259 recommended Roblox Open Cloud resource
  operations across 28 domains.
- Endpoint metadata for stability, authentication support, and OAuth scopes.
- A reproducible Creator Docs coverage inventory and synchronization script.
- Strict Rust and Clippy linting, HTTP contract tests, documentation checks,
  and package verification.
