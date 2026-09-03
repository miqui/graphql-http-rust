# graphql-http-rust

A Rust implementation of the HTTP-layer behavior defined by the
[GraphQL-over-HTTP specification](https://github.com/graphql/graphql-over-http)
(canonical spec text vendored at `spec/GraphQLOverHTTP.md`).

## Layout

- `graphql-http/` — core library: content negotiation (`media`), request
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

Not published to crates.io (`publish = false` at the workspace level).
