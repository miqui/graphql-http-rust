# graphql-http-rust

[![CI](https://github.com/miqui/graphql-http-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/miqui/graphql-http-rust/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/miqui/graphql-http-rust/blob/main/LICENSE)
![crates.io](https://img.shields.io/crates/v/graphql-http-rust.svg)
[![docs.rs](https://img.shields.io/docsrs/graphql-http-rust)](https://docs.rs/graphql-http-rust)
![Rust: stable](https://img.shields.io/badge/rust-stable-orange.svg)
![Spec: GraphQL over HTTP, Stage 2 Draft](https://img.shields.io/badge/spec-GraphQL%20over%20HTTP%20%28Stage%202%20Draft%29-8A2BE2.svg)

A Rust implementation of the HTTP-layer behavior defined by the
[GraphQL-over-HTTP specification](https://github.com/graphql/graphql-over-http)
(Stage 2 Draft).

> **⚠️ Status: draft — not ready for production.** This is a reference
> implementation of a **Stage 2 Draft** spec. The draft may still change,
> occasionally dramatically, and is not guaranteed to be accepted. The API may
> break at any time while the spec evolves. Suitable for experimentation,
> conformance testing, and review — not for production GraphQL services.

## What it implements

Only the **HTTP transport layer** of the spec — deliberately no GraphQL
parsing, validation, or execution (those belong to your GraphQL engine):

- Content negotiation between the spec media type
  (`application/graphql-response+json`) and legacy `application/json`,
  including q-value ordering and wildcard `Accept` handling
- Request parsing: POST JSON bodies and GET query parameters, with the spec's
  forward-compatibility rule (unknown properties ignored)
- Response encoding: 200 / 294 partial-success / request-error status rules,
  and the legacy-client `Content-Type` downgrade
- Status-code decision helpers mapping every spec-listed failure condition to
  its RECOMMENDED status code (405/415/406/400/422/503, …)

## Usage

Run the end-to-end example:

```sh
cargo run --example usage
```

See the [repository README](https://github.com/miqui/graphql-http-rust) for
the full layout, the axum example server, the k6 load suite, and the
spec-mapped test suite (every test names the spec section it verifies).

## License

MIT — see [LICENSE](https://github.com/miqui/graphql-http-rust/blob/main/LICENSE).
