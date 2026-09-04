//! End-to-end usage examples for the `graphql-http` crate.
//!
//! Run with: `cargo run --example usage`

use std::collections::HashMap;

use graphql_http::{
    document_looks_like_mutation, encode_response, negotiate, parse_get_params, parse_json_body,
    GraphQLRequest, GraphQLResult, HttpResponse, Negotiated, RequestFailure, RequestParseError,
};

fn main() {
    // =====================================================================
    // 1. Content negotiation — decide the response media type from Accept
    // =====================================================================
    // Spec-compliant client asks for the spec media type:
    let negotiated = negotiate(Some("application/graphql-response+json"));
    assert!(matches!(negotiated, Negotiated::GraphqlResponseJson));

    // Legacy client only speaks application/json:
    let legacy = negotiate(Some("application/json"));
    assert!(matches!(legacy, Negotiated::LegacyJson));

    // No Accept header at all → default to the spec media type.
    assert!(matches!(negotiate(None), Negotiated::GraphqlResponseJson));

    // Client wants something we can't produce, and not even legacy JSON →
    // the server should reply 406 (Not Acceptable).
    assert!(matches!(
        negotiate(Some("text/html")),
        Negotiated::NotAcceptable
    ));

    // =====================================================================
    // 2. Parse a POST request body into a GraphQLRequest
    // =====================================================================
    // A well-formed POST body: {"query": "...", "variables": {...}, "operationName": "..."}
    let body = br#"{"query": "query Q($i: Int!) { q(i: $i) }", "variables": {"i": 7}}"#;

    let request: GraphQLRequest = match parse_json_body(body) {
        Ok(req) => req,
        Err(RequestParseError::NotParsable(_)) => {
            // Malformed JSON → 400 Bad Request (spec: "JSON parsing failure")
            unreachable!()
        }
        Err(RequestParseError::NotWellFormed(_)) => {
            // Parsed but wrong shape → 422 Unprocessable Content
            unreachable!()
        }
        Err(e) => {
            // Anything else (e.g. MutationViaGet on the GET path) → use
            // RequestFailure::recommended_status() to pick the status code.
            unreachable!("{e}")
        }
    };
    assert_eq!(request.query, "query Q($i: Int!) { q(i: $i) }");
    // Unknown extra properties in the JSON body were silently ignored,
    // exactly as the spec's forward-compatibility rule requires.

    // =====================================================================
    // 3. Parse GET query parameters (query params are percent-decoded
    //    by your web framework before they reach this function)
    // =====================================================================
    let mut params: HashMap<String, String> = HashMap::new();
    params.insert("query".to_string(), "{ __typename }".to_string());

    let get_request: GraphQLRequest = parse_get_params(&params).expect("well-formed GET");
    assert_eq!(get_request.query, "{ __typename }");

    // Mutations MUST NOT be executed via GET (spec: `#sec-GET`) → the
    // application checks the heuristic BEFORE executing and rejects with
    // 405 + `Allow: POST` header (RFC 9110 requires Allow on 405).
    let mutation_query = "mutation { createUser }";
    assert!(document_looks_like_mutation(mutation_query, None));
    // (In the example-server this becomes:
    //   if document_looks_like_mutation(&req.query, req.operation_name.as_deref()) {
    //       return Response::builder().status(405).header("Allow", "POST")...
    //   })

    // =====================================================================
    // 4. Build a GraphQL execution result and encode the HTTP response
    // =====================================================================
    // Full success — data only → status 200:
    let result = GraphQLResult::data_only(serde_json::json!({ "q": 42 }));
    let response: HttpResponse = encode_response(&result, negotiated);
    // → 200, Content-Type: application/graphql-response+json; charset=utf-8
    println!(
        "success: {} {} {}",
        response.status,
        response.content_type,
        String::from_utf8_lossy(&response.body)
    );

    // Partial success — data present (even null) plus errors → 294:
    let partial = GraphQLResult::partial(
        serde_json::json!(null),
        vec![serde_json::json!({"message": "field 'q' failed"})],
    );
    let response = encode_response(&partial, negotiated);
    // → 294 (STATUS_PARTIAL_SUCCESS), still the spec media type
    println!("partial: {} {}", response.status, response.content_type);

    // Request error — no data entry at all → 4xx/5xx, never the spec type:
    let error = GraphQLResult::request_error(vec![serde_json::json!({
        "message": "unknown field"
    })]);
    // The HTTP-layer crate defaults the status; the application overrides it
    // using the failure classification from §5 (e.g. 422 for validation
    // failures), and the response body is the GraphQL error result.
    let error_response = encode_response(&error, negotiated);
    println!(
        "request-error body: {}",
        String::from_utf8_lossy(&error_response.body)
    );

    // =====================================================================
    // 5. Map a pre-execution failure to its RECOMMENDED status code
    // =====================================================================
    let failures = [
        (RequestFailure::MutationViaGet, 405),
        (RequestFailure::UnsupportedContentType, 415),
        (RequestFailure::NotAcceptable, 406),
        (RequestFailure::JsonParseFailure, 400),
        (RequestFailure::NotWellFormedRequest, 422),
        (RequestFailure::ValidationFailure, 422),
        (RequestFailure::ServiceUnavailable, 503),
    ];
    for (failure, expected) in failures {
        assert_eq!(failure.recommended_status(), expected);
    }

    // The mutation heuristic used by the GET path is also public:
    assert!(document_looks_like_mutation(
        "mutation { createUser }",
        None
    ));

    println!("all examples ran ✓");
}
