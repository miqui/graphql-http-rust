# graphql-http-rust

[![CI](https://github.com/miqui/graphql-http-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/miqui/graphql-http-rust/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![crates.io](https://img.shields.io/crates/v/graphql-http-rust.svg)](https://crates.io/crates/graphql-http-rust)
[![docs.rs](https://img.shields.io/docsrs/graphql-http-rust)](https://docs.rs/graphql-http-rust)
![Rust: stable](https://img.shields.io/badge/rust-stable-orange.svg)
![Spec: GraphQL over HTTP, Stage 2 Draft](https://img.shields.io/badge/spec-GraphQL%20over%20HTTP%20%28Stage%202%20Draft%29-8A2BE2.svg)

A Rust implementation of the HTTP-layer behavior defined by the
[GraphQL-over-HTTP specification](https://github.com/graphql/graphql-over-http)
(canonical spec text vendored at `spec/GraphQLOverHTTP.md`).

> **⚠️ Status: draft — not ready for production.** This is a reference
> implementation of the **GraphQL-over-HTTP spec at Stage 2: Draft**. The
> draft may still change, occasionally dramatically, and is not guaranteed to
> be accepted. The crate's API may break at any time while the spec evolves.
> Suitable for experimentation, conformance testing, and review — not for
> production GraphQL services.

## Layout

- `graphql-http/` — the publishable crate (`graphql-http-rust` on
  crates.io): content negotiation (`media`), request
  parsing for GET/POST (`request`), `GraphQLResult`/response encoding
  (`response`), and HTTP status-code decision helpers (`status`). This crate
  implements only the HTTP transport layer; GraphQL parsing, validation, and
  execution are the responsibility of the application/GraphQL engine.
- `examples/example-server/` — a toy Axum server exercising the library
  end-to-end (GET/POST, both `application/graphql-response+json` and legacy
  `application/json` negotiation, 200/294/422/405/406/415 status paths).

## Scope

Both spec-compliant mode (`Accept`/`Content-Type: application/graphql-response+json`)
and legacy-compat mode (`application/json`) are implemented, including the
"except 2xx" `Content-Type` downgrade rule for legacy clients (spec: "Body").

## Testing

Every test function name and doc comment cites the spec section it verifies,
e.g. `status_codes_sec_status_codes_partial_data_and_errors_yields_294`
verifies the "Status Codes" (`#sec-Status-Codes`) 294 partial-success rule.

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Running the example server

```sh
cargo run -p example-server
# then, e.g.:
curl -X POST http://127.0.0.1:8080/graphql \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/graphql-response+json' \
  -d '{"query": "{ hello }"}'
```

## Conformance testing with k6

[`examples/k6/graphql-scenarios.js`](examples/k6/graphql-scenarios.js) is a
[k6](https://k6.io) suite that tests the example server's compliance with the
GraphQL-over-HTTP spec over real HTTP, then load-tests it. Every assertion is
tagged with the spec section it verifies, and a single
`graphql_spec_conformance rate==1.0` threshold fails the run if any spec
assertion breaks.

Install k6 (macOS):

```sh
brew install k6
```

Start the server, then run the suite in another terminal:

```sh
cargo run -p example-server        # listens on http://127.0.0.1:8080/graphql
k6 run examples/k6/graphql-scenarios.js
```

Scenarios (run all, or pick one with `--env SCENARIO=<name>`):

- `smoke` — 16-assertion conformance pass over every documented status path
  (200, 294, 422, 415, 406, 405+`Allow`, 400; legacy `application/json`
  downgrade; wildcard/absent Accept; unknown-property tolerance). Fast fail:
  run this before any load scenario.
- `query_load` — 200 req/s constant-arrival happy-path throughput.
- `mixed_ramp` — realistic traffic mix (queries, mutations, GET, partial
  results) ramping 5 → 150 VUs.
- `error_path_spike` — burst of malformed/rejected requests to confirm the
  rejection paths stay cheap under pressure.

```sh
k6 run --env SCENARIO=smoke examples/k6/graphql-scenarios.js
k6 run --env BASE_URL=http://localhost:3000 examples/k6/graphql-scenarios.js
```

A passing full run reports `graphql_spec_conformance: 100.00%` across all
scenarios; any drift from the spec fails the run via threshold.

Not published to crates.io (`publish = false` at the workspace level).
