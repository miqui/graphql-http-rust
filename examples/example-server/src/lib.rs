//! Example server exercising `graphql-http` end-to-end over Axum.
//!
//! This is a toy "GraphQL service" (it does not implement full GraphQL
//! parsing/validation/execution — see `execute_toy` below) whose sole
//! purpose is to demonstrate wiring the `graphql-http` crate's content
//! negotiation, request parsing, response encoding, and status-code
//! decision logic into a real HTTP server.
//!
//! Supported toy operations (matched by trimmed query string):
//! - `{ hello }` -> `{"data": {"hello": "world"}}`, HTTP 200
//! - `{ partial }` -> data + errors -> HTTP 294
//! - `{ boom }` -> GraphQL request error (no {data}) -> HTTP 422
//! - `mutation { createThing }` -> `{"data": {"createThing": true}}` when
//!   executed via POST; MUST NOT be executed via GET (HTTP 405).

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::Response,
    routing::get,
    Router,
};
use graphql_http_rust::{
    document_looks_like_mutation, encode_response, negotiate, parse_get_params, parse_json_body,
    GraphQLRequest, GraphQLResult, Negotiated, RequestParseError,
};
use serde_json::json;
#[cfg(test)]
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone)]
struct AppState;

/// Builds the example service's Axum router. Exposed as `pub` so both the
/// `example-server` binary (`main.rs`) and wire-level integration tests
/// (`tests/wire.rs`, which bind it to a real ephemeral TCP port and drive it
/// with `reqwest`) can construct and serve the same router.
pub fn build_router() -> Router {
    Router::new()
        .route("/graphql", get(handle_get).post(handle_post))
        .with_state(AppState)
}

/// Naive check for whether a "document" fails to parse, for the purposes of
/// demonstrating the spec's "Document parsing failure" example
/// (`#sec-Status-Codes`): a POST body of `{"query": "{"}"` (unbalanced
/// braces) should yield 400. This is not a real GraphQL parser; it only
/// checks brace balance as a stand-in for the toy service.
fn document_parse_error(query: &str) -> bool {
    let mut depth: i32 = 0;
    for c in query.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return true;
        }
    }
    depth != 0
}

/// Executes the toy "GraphQL service". Not a real GraphQL engine.
fn execute_toy(request: &GraphQLRequest) -> GraphQLResult {
    let trimmed = request.query.trim();
    if trimmed.contains("boom") {
        GraphQLResult::request_error(vec![
            json!({"message": "Cannot query field \"boom\" on type \"Query\"."}),
        ])
    } else if trimmed.contains("partial") {
        GraphQLResult::partial(
            json!({"partial": null}),
            vec![json!({"message": "partial field failed"})],
        )
    } else if trimmed.contains("createThing") {
        GraphQLResult::data_only(json!({"createThing": true}))
    } else {
        GraphQLResult::data_only(json!({"hello": "world"}))
    }
}

fn accept_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Converts a `graphql_http_rust::HttpResponse` into an Axum `Response`.
fn to_axum_response(resp: graphql_http_rust::HttpResponse) -> Response {
    let status = StatusCode::from_u16(resp.status).unwrap_or(StatusCode::OK);
    let mut response = Response::builder().status(status);
    if let Ok(hv) = HeaderValue::from_str(&resp.content_type) {
        response = response.header(axum::http::header::CONTENT_TYPE, hv);
    }
    response.body(axum::body::Body::from(resp.body)).unwrap()
}

/// Builds a request-error `HttpResponse` for failures that occur before we
/// even have a `GraphQLRequest`/`GraphQLResult` (e.g. bad JSON, not
/// well-formed, method misuse). Spec: "Status Codes" (`#sec-Status-Codes`).
fn error_response(status: u16, message: &str, extra_headers: &[(&str, &str)]) -> Response {
    let body = serde_json::to_vec(&json!({ "errors": [{ "message": message }] })).unwrap();
    let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    let mut builder = Response::builder().status(status_code).header(
        axum::http::header::CONTENT_TYPE,
        "application/graphql-response+json; charset=utf-8",
    );
    for (k, v) in extra_headers {
        builder = builder.header(*k, *v);
    }
    builder.body(axum::body::Body::from(body)).unwrap()
}

async fn handle_post(State(_state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());

    // Spec: "POST" (`#sec-POST`) - server MUST support application/json;
    // if no Content-Type is supplied, SHOULD reject with 4xx.
    let Some(content_type) = content_type else {
        return error_response(400, "Content-Type header is required", &[]);
    };
    let essence = graphql_http_rust::media::content_type_essence(content_type);
    if essence != "application/json"
        && essence != graphql_http_rust::APPLICATION_GRAPHQL_RESPONSE_JSON
    {
        return error_response(415, "unsupported Content-Type", &[]);
    }

    let accept = accept_header(&headers);
    let negotiated = negotiate(accept.as_deref());
    if negotiated == Negotiated::NotAcceptable {
        return error_response(406, "no acceptable media type", &[]);
    }

    let request = match parse_json_body(&body) {
        Ok(r) => r,
        Err(RequestParseError::NotParsable(msg)) => {
            return error_response(400, &format!("could not parse JSON body: {msg}"), &[])
        }
        Err(RequestParseError::NotWellFormed(msg)) => {
            return error_response(422, &format!("not a well-formed request: {msg}"), &[])
        }
        Err(RequestParseError::MutationViaGet) => unreachable!("POST cannot hit this variant"),
    };

    let result = execute_toy(&request);
    if document_parse_error(&request.query) {
        return error_response(400, "GraphQL document could not be parsed", &[]);
    }
    respond_with_result(&result, negotiated)
}

async fn handle_get(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let accept = accept_header(&headers);
    let negotiated = negotiate(accept.as_deref());
    if negotiated == Negotiated::NotAcceptable {
        return error_response(406, "no acceptable media type", &[]);
    }

    let request = match parse_get_params(&params) {
        Ok(r) => r,
        Err(RequestParseError::NotWellFormed(msg)) => {
            return error_response(422, &format!("not a well-formed request: {msg}"), &[])
        }
        Err(RequestParseError::NotParsable(msg)) => return error_response(400, &msg, &[]),
        Err(RequestParseError::MutationViaGet) => unreachable!(),
    };

    // Spec: "GET" (`#sec-GET`) - GET MUST NOT execute mutation operations;
    // 405 RECOMMENDED with an Allow header.
    if document_looks_like_mutation(&request.query, request.operation_name.as_deref()) {
        return error_response(
            405,
            "mutations must not be executed via GET",
            &[("Allow", "POST")],
        );
    }

    let result: GraphQLResult = execute_toy(&request);
    respond_with_result(&result, negotiated)
}

/// Renders a `GraphQLResult` to an HTTP response. When the result has no
/// {data} entry it is a _GraphQL request error result_ (spec: "Status
/// Codes", `#sec-Status-Codes`), which MUST use a 4xx/5xx status code; our
/// toy executor's own errors are treated as validation-style failures
/// (`422`). Successful/partial results are encoded via `encode_response`,
/// which applies the spec's status/Content-Type decision logic.
fn respond_with_result(result: &GraphQLResult, negotiated: Negotiated) -> Response {
    if !result.has_data() {
        let mut resp = encode_response(result, negotiated);
        resp.status = graphql_http_rust::RequestFailure::ValidationFailure.recommended_status();
        return to_axum_response(resp);
    }
    to_axum_response(encode_response(result, negotiated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn body_json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Spec: "POST" (`#sec-POST`) + "Status Codes" (`#sec-Status-Codes`) -
    /// a well-formed POST request with application/json content type and no
    /// errors yields HTTP 200 with {data}.
    #[tokio::test]
    async fn post_sec_post_well_formed_request_yields_200_with_data() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from(r#"{"query": "{ hello }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(content_type.starts_with("application/graphql-response+json"));
        let body = body_json(response).await;
        assert_eq!(body["data"]["hello"], "world");
    }

    /// Spec: "Body" (`#sec-Body`) - legacy client (Accept: application/json)
    /// on a 2xx result gets Content-Type: application/json.
    #[tokio::test]
    async fn body_sec_body_legacy_client_2xx_gets_application_json_content_type() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(axum::body::Body::from(r#"{"query": "{ hello }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(content_type.starts_with("application/json"));
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) - a request error result
    /// (no {data}) yields the recommended 422 status.
    #[tokio::test]
    async fn status_codes_sec_status_codes_request_error_yields_422() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from(r#"{"query": "{ boom }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) / "Partial success"
    /// (`#sec-Partial-success`) - data + errors yields custom 294 status.
    #[tokio::test]
    async fn status_codes_sec_status_codes_partial_result_yields_294() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from(r#"{"query": "{ partial }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status().as_u16(), 294);
    }

    /// Spec: "POST" (`#sec-POST`) - missing Content-Type SHOULD be rejected
    /// with an appropriate 4xx.
    #[tokio::test]
    async fn post_sec_post_missing_content_type_is_rejected() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .body(axum::body::Body::from(r#"{"query": "{ hello }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert!(response.status().is_client_error());
    }

    /// Spec: "GET" (`#sec-GET`) - GET request with well-formed query params
    /// executes successfully.
    #[tokio::test]
    async fn get_sec_get_well_formed_query_executes_successfully() {
        let app = build_router();
        let req = Request::builder()
            .method("GET")
            .uri("/graphql?query=%7B%20hello%20%7D")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["data"]["hello"], "world");
    }

    /// Spec: "GET" (`#sec-GET`) - mutation via GET MUST fail with 4xx (405
    /// RECOMMENDED), with an Allow header present.
    #[tokio::test]
    async fn get_sec_get_mutation_via_get_yields_405_with_allow_header() {
        let app = build_router();
        let req = Request::builder()
            .method("GET")
            .uri("/graphql?query=mutation%20%7B%20createThing%20%7D")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert!(response.headers().get("allow").is_some());
    }

    /// Spec: "Body" (`#sec-Body`) - Accept header naming an unsupported type
    /// yields 406 Not Acceptable.
    #[tokio::test]
    async fn body_sec_body_unsupported_accept_yields_406() {
        let app = build_router();
        let req = Request::builder()
            .method("GET")
            .uri("/graphql?query=%7B%20hello%20%7D")
            .header("accept", "text/plain")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
    }

    /// Spec: "GET" (`#sec-GET`) - missing required {query} param is not a
    /// well-formed request, 422 RECOMMENDED.
    #[tokio::test]
    async fn get_sec_get_missing_query_yields_422() {
        let app = build_router();
        let req = Request::builder()
            .method("GET")
            .uri("/graphql")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// Spec: "POST" (`#sec-POST`) - unsupported Content-Type yields 415.
    #[tokio::test]
    async fn post_sec_post_unsupported_content_type_yields_415() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "text/plain")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from(r#"{"query": "{ hello }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    /// Spec: "Media Types" (`#sec-Media-Types`) - "If the media type in a
    /// `Content-Type` or `Accept` header does not include encoding
    /// information and matches one of the officially recognized GraphQL
    /// media types, then `utf-8` MUST be assumed." A `Content-Type` of
    /// `application/json` with no charset parameter must still be accepted
    /// and processed as UTF-8, not rejected as unsupported.
    #[tokio::test]
    async fn media_types_sec_media_types_missing_charset_assumes_utf8() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from(r#"{"query": "{ hello }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// Spec: "Accept" (`#sec-Accept`) - a client MAY omit the `Accept`
    /// header entirely; the server MAY respond with any content type of its
    /// choosing. We choose the spec-compliant media type by default.
    #[tokio::test]
    async fn accept_sec_accept_absent_header_still_succeeds() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"query": "{ hello }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(content_type.starts_with("application/graphql-response+json"));
    }

    /// Spec: "Accept" (`#sec-Accept`) - a wildcard `*/*` Accept header is
    /// honored and negotiates the spec-compliant media type.
    #[tokio::test]
    async fn accept_sec_accept_wildcard_negotiates_graphql_response_json() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "*/*")
            .body(axum::body::Body::from(r#"{"query": "{ hello }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(content_type.starts_with("application/graphql-response+json"));
    }

    /// Spec: "Body" (`#sec-Body`) - "If the `Accept` header is present but
    /// does not indicate support for any of the server's supported media
    /// types or `application/json`, the server SHOULD respond with `406`."
    /// This exercises the 406 path via POST (in addition to the existing
    /// GET-based 406 test) to confirm the same behavior on both methods.
    #[tokio::test]
    async fn body_sec_body_accept_excludes_both_supported_types_yields_406_on_post() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/xml, text/plain;q=0.5")
            .body(axum::body::Body::from(r#"{"query": "{ hello }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
    }

    /// Spec: "Body" (`#sec-Body`) - legacy client accommodation: when the
    /// `Accept` header does not include `application/graphql-response+json`
    /// but does include `application/json`, the request is processed and a
    /// 2xx result gets `Content-Type: application/json`, while the JSON
    /// body shape (containing {data}) is unaffected — i.e. legacy mode
    /// downgrades the header, not the payload.
    #[tokio::test]
    async fn body_sec_body_legacy_mode_downgrades_content_type_but_not_payload() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/json;q=0.9")
            .body(axum::body::Body::from(r#"{"query": "{ hello }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(content_type.starts_with("application/json"));
        let body = body_json(response).await;
        assert_eq!(body["data"]["hello"], "world");
    }

    /// Spec: "Body" (`#sec-Body`) - legacy mode's `Content-Type` downgrade
    /// rule is keyed on the HTTP status code being in the `2xx` range; the
    /// partial-success `294` status code falls within that numeric range
    /// (200-299), so per this crate's implementation it also receives the
    /// `application/json` downgrade for a legacy client's 2xx-range
    /// response, even though the result contains {errors}.
    #[tokio::test]
    async fn body_sec_body_legacy_mode_294_in_2xx_range_gets_json_content_type() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(axum::body::Body::from(r#"{"query": "{ partial }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status().as_u16(), 294);
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(content_type.starts_with("application/json"));
    }

    /// Spec: "JSON parsing failure" example (`#sec-Status-Codes`) - a POST
    /// body that is not valid JSON at all (e.g. `NONSENSE`) must yield 400.
    #[tokio::test]
    async fn status_codes_sec_status_codes_malformed_json_body_yields_400() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from("NONSENSE"))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// Spec: "Invalid parameters" example (`#sec-Status-Codes`) - a typo'd
    /// property name (`qeury` instead of `query`) means the required
    /// `query` parameter is absent, which is not a well-formed request and
    /// yields 422.
    #[tokio::test]
    async fn status_codes_sec_status_codes_typo_qeury_property_yields_422() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from(r#"{"qeury": "{__typename}"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// Spec: "Invalid parameters" example (`#sec-Status-Codes`) - the exact
    /// spec example of an invalid shape for `variables` (an array instead
    /// of a map) yields 422.
    #[tokio::test]
    async fn status_codes_sec_status_codes_variables_wrong_shape_yields_422() {
        let app = build_router();
        let body = r#"{"query": "query Q ($i:Int!) { q(i: $i) }", "variables": [7]}"#;
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from(body))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// Spec: "JSON Encoding" (`#sec-JSON-Encoding`) - "Servers receiving a
    /// request with additional properties MUST ignore properties they do
    /// not understand." Extra unknown top-level properties don't prevent a
    /// well-formed request from executing successfully.
    #[tokio::test]
    async fn json_encoding_sec_json_encoding_extra_unknown_properties_ignored() {
        let app = build_router();
        let body = r#"{"query": "{ hello }", "somethingUnknown": 42, "another": {"x": 1}}"#;
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from(body))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["data"]["hello"], "world");
    }

    /// Spec: "Document parsing failure" example (`#sec-Status-Codes`) - the
    /// exact spec example `{"query": "{"}` (a document that cannot be
    /// parsed, here detected via the toy server's brace-balance check)
    /// yields 400.
    #[tokio::test]
    async fn status_codes_sec_status_codes_document_parse_failure_yields_400() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from(r#"{"query": "{"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) / "Partial success"
    /// (`#sec-Partial-success`) - a partial-success result includes both a
    /// `data` entry (here `null`) and a non-empty `errors` entry in the
    /// response body, alongside the custom 294 status code.
    #[tokio::test]
    async fn status_codes_sec_status_codes_294_body_contains_null_data_and_errors() {
        let app = build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(axum::body::Body::from(r#"{"query": "{ partial }"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status().as_u16(), 294);
        let body = body_json(response).await;
        assert_eq!(body["data"]["partial"], Value::Null);
        assert!(body["errors"].is_array());
        assert!(!body["errors"].as_array().unwrap().is_empty());
    }
}
