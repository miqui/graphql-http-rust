//! Wire-level HTTP integration tests for the example server.
//!
//! Unlike `src/lib.rs`'s in-process `tower::ServiceExt::oneshot` tests
//! (which never touch a real socket), these tests spawn the *actual* Axum
//! server via `axum::serve` bound to TCP port 0 (an OS-assigned ephemeral
//! port), then drive it over real HTTP with `reqwest`. This closes the gap
//! where content negotiation, status codes, and headers were only ever
//! validated in-process.
//!
//! Design choice: rather than shelling out to `cargo run` (slow, fragile
//! readiness polling, awkward port discovery), the example-server crate
//! exposes `build_router()` from `src/lib.rs`. This test binds that same
//! router to `127.0.0.1:0`, reads back the OS-assigned port via
//! `TcpListener::local_addr()`, and spawns `axum::serve` on a background
//! Tokio task for the duration of each test.

use example_server::build_router;
use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;

/// Spawns the real example server on an ephemeral port and returns its base
/// URL (e.g. `http://127.0.0.1:54321`). The server task runs for the
/// lifetime of the test process; each test gets its own instance bound to
/// its own OS-assigned port, so tests can run in parallel without clashing.
async fn spawn_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind ephemeral port");
    let addr = listener.local_addr().expect("failed to read local_addr");
    let app = build_router();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("axum::serve exited unexpectedly");
    });
    format!("http://{addr}/graphql")
}

/// Spec: "POST" (`#sec-POST`) + "Status Codes" (`#sec-Status-Codes`) - a
/// real-HTTP POST with `Accept: application/graphql-response+json` yields
/// 200, the correct Content-Type, and `data.hello == "world"`.
#[tokio::test]
async fn post_sec_post_wire_hello_yields_200_with_correct_content_type() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(r#"{"query": "{ hello }"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(content_type.starts_with("application/graphql-response+json"));
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["data"]["hello"], "world");
}

/// Spec: "Status Codes" (`#sec-Status-Codes`) / "Partial success"
/// (`#sec-Partial-success`) - a real-HTTP request for `{ partial }` yields
/// the custom 294 status with `data.partial == null` and non-empty errors.
#[tokio::test]
async fn status_codes_sec_status_codes_wire_partial_yields_294() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(r#"{"query": "{ partial }"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 294);
    let body: Value = response.json().await.unwrap();
    assert!(body["data"]["partial"].is_null());
    assert!(body["errors"].as_array().is_some_and(|e| !e.is_empty()));
}

/// Spec: "Status Codes" (`#sec-Status-Codes`) - a real-HTTP request-error
/// result (no {data}, e.g. `{ boom }`) yields 422 with no `data` field and
/// non-empty `errors`.
#[tokio::test]
async fn status_codes_sec_status_codes_wire_boom_yields_422_no_data() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(r#"{"query": "{ boom }"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = response.json().await.unwrap();
    assert!(body.get("data").is_none());
    assert!(body["errors"].as_array().is_some_and(|e| !e.is_empty()));
}

/// Spec: "Status Codes" (`#sec-Status-Codes`) - malformed JSON in the
/// request body (not parsable at all) yields 400 over real HTTP.
#[tokio::test]
async fn status_codes_sec_status_codes_wire_malformed_json_yields_400() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body("NONSENSE")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Spec: "Status Codes" (`#sec-Status-Codes`) - a well-formed request whose
/// `{query}` document fails to parse (unbalanced braces, per the toy
/// server's `document_parse_error` stand-in) yields 400 over real HTTP.
#[tokio::test]
async fn status_codes_sec_status_codes_wire_document_parse_failure_yields_400() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(r#"{"query": "{"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Spec: "POST" (`#sec-POST`) - an unsupported `Content-Type` (e.g.
/// `text/plain`) yields 415 over real HTTP.
#[tokio::test]
async fn post_sec_post_wire_unsupported_content_type_yields_415() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("content-type", "text/plain")
        .header("accept", "application/graphql-response+json")
        .body(r#"{"query": "{ hello }"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

/// Spec: "Body" (`#sec-Body`) - an `Accept` header naming an unsupported
/// media type (e.g. `application/xml`) yields 406 Not Acceptable over real
/// HTTP.
#[tokio::test]
async fn body_sec_body_wire_unsupported_accept_yields_406() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/xml")
        .body(r#"{"query": "{ hello }"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
}

/// Spec: "Body" (`#sec-Body`) - a legacy client sending
/// `Accept: application/json` on a 2xx result gets the response downgraded
/// to `Content-Type: application/json` (instead of
/// `application/graphql-response+json`), still with HTTP 200, over real
/// HTTP.
#[tokio::test]
async fn body_sec_body_wire_legacy_accept_downgrades_content_type() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(r#"{"query": "{ hello }"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(content_type.starts_with("application/json"));
    assert!(!content_type.starts_with("application/graphql-response+json"));
}

/// Spec: "GET" (`#sec-GET`) - a real-HTTP GET with a well-formed `query`
/// param executes successfully (200).
#[tokio::test]
async fn get_sec_get_wire_well_formed_query_yields_200() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{url}?query={{%20hello%20}}"))
        .header("accept", "application/graphql-response+json")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["data"]["hello"], "world");
}

/// Spec: "GET" (`#sec-GET`) - GET MUST NOT execute mutation operations;
/// over real HTTP this yields 405 with an `Allow: POST` header value.
#[tokio::test]
async fn get_sec_get_wire_mutation_yields_405_with_allow_post() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{url}?query=mutation%20%7B%20createThing%20%7D"))
        .header("accept", "application/graphql-response+json")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let allow = response
        .headers()
        .get(reqwest::header::ALLOW)
        .expect("Allow header must be present")
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(allow, "POST");
}

/// Spec: "GET" (`#sec-GET`) - a GET request missing the required `query`
/// param is not well-formed and yields 422 over real HTTP.
#[tokio::test]
async fn get_sec_get_wire_missing_query_yields_422() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("accept", "application/graphql-response+json")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Spec: "JSON Encoding" (`#sec-JSON-Encoding`) - unknown/extra properties
/// in the JSON request body are ignored by the server (200), over real
/// HTTP.
#[tokio::test]
async fn json_encoding_sec_json_encoding_wire_unknown_properties_ignored() {
    let url = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(json!({"query": "{ hello }", "unknownField": "should be ignored"}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["data"]["hello"], "world");
}
