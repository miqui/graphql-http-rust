//! `graphql-http-rust`: HTTP-layer implementation of the GraphQL-over-HTTP
//! specification.
//!
//! **Status: draft.** This crate tracks the GraphQL-over-HTTP specification
//! at **Stage 2: Draft** — the draft may still change, occasionally
//! dramatically, and is not guaranteed to be accepted. Treat this crate as a
//! draft-spec reference implementation: suitable for experimentation,
//! conformance testing, and review — **not ready for production use**.
//!
//! This crate implements only the **HTTP transport layer** behavior defined
//! by the [GraphQL-over-HTTP specification](https://github.com/graphql/graphql-over-http):
//! content negotiation, request parsing (GET/POST, JSON encoding), response
//! encoding, and HTTP status-code decision logic. It deliberately does not
//! implement GraphQL parsing, validation, or execution — those are the
//! responsibility of a GraphQL engine (e.g. `async-graphql`, `juniper`) that
//! an application wires up alongside this crate. See `spec/GraphQLOverHTTP.md`
//! in the repository root for the canonical specification text this crate
//! implements against.
//!
//! Module map:
//! - [`media`]: media type parsing & content negotiation (`Accept`/`Content-Type`).
//! - [`request`]: parsing GET query params / POST JSON bodies into a
//!   well-formed `GraphQLRequest`.
//! - [`response`]: `GraphQLResult` type, response status decision, and
//!   response encoding honoring negotiated content type.
//! - [`status`]: HTTP status-code recommendations for request-level failure
//!   conditions (pre-execution).

pub mod media;
pub mod request;
pub mod response;
pub mod status;

pub use media::{
    negotiate, MediaRange, Negotiated, APPLICATION_GRAPHQL_RESPONSE_JSON, APPLICATION_JSON,
};
pub use request::{
    document_looks_like_mutation, parse_get_params, parse_json_body, GraphQLRequest,
    RequestParseError,
};
pub use response::{
    decide_result_status, encode_response, GraphQLResult, HttpResponse, StatusDecision,
    STATUS_PARTIAL_SUCCESS,
};
pub use status::RequestFailure;
