//! GraphQL result / GraphQL-over-HTTP response representation and encoding.
//!
//! Spec: "Response" (`#sec-Response`), "Body" (`#sec-Body`), "Status Codes"
//! (`#sec-Status-Codes`).

use crate::media::{Negotiated, APPLICATION_GRAPHQL_RESPONSE_JSON, APPLICATION_JSON};
use serde::Serialize;
use serde_json::Value;

/// A _GraphQL result_: describes the result of parsing, validating, and (if
/// successful) executing a requested operation.
///
/// Spec: "Response" (`#sec-Response`) - "A _GraphQL result_ describes the
/// result of parsing, validating and (if successful) executing the
/// requested operation, and any errors encountered during the request."
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphQLResult {
    /// {data} entry. `None` means the entry is entirely absent (a _GraphQL
    /// request error result_); `Some(Value::Null)` means present-but-null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    /// {errors} entry, a non-empty list when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<Value>>,
    /// {extensions} entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Value>,
}

impl GraphQLResult {
    /// Builds a successful execution result with only {data}.
    pub fn data_only(data: Value) -> Self {
        Self {
            data: Some(data),
            errors: None,
            extensions: None,
        }
    }

    /// Builds a partial-success execution result with both {data} (which
    /// may be `null`) and non-empty {errors}.
    pub fn partial(data: Value, errors: Vec<Value>) -> Self {
        Self {
            data: Some(data),
            errors: Some(errors),
            extensions: None,
        }
    }

    /// Builds a _GraphQL request error result_: no {data} entry at all.
    pub fn request_error(errors: Vec<Value>) -> Self {
        Self {
            data: None,
            errors: Some(errors),
            extensions: None,
        }
    }

    /// Whether this result contains the {data} entry (regardless of value).
    pub fn has_data(&self) -> bool {
        self.data.is_some()
    }

    /// Whether {data} is present and non-null.
    pub fn has_non_null_data(&self) -> bool {
        matches!(&self.data, Some(v) if !v.is_null())
    }

    /// Whether the {errors} entry is present.
    pub fn has_errors(&self) -> bool {
        self.errors.is_some()
    }
}

/// The decided HTTP status code and category for a `GraphQLResult`,
/// independent of framework specifics.
///
/// Spec: "Status Codes" (`#sec-Status-Codes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusDecision {
    pub code: u16,
}

/// A custom, non-IANA-registered status code recommended by the spec for
/// partial success. Spec: "Partial success" (`#sec-Partial-success`) and
/// "Status Codes" (`#sec-Status-Codes`).
pub const STATUS_PARTIAL_SUCCESS: u16 = 294;

/// Decides the HTTP status code for a well-formed _GraphQL-over-HTTP
/// response_ body (i.e. once we know we have a `GraphQLResult`, as opposed
/// to a request-level failure that never reached execution).
///
/// Spec: "Status Codes" (`#sec-Status-Codes`):
/// - {data} present and non-null => MUST be `2xx` (we choose `200`).
/// - {data} present, non-null, and no {errors} => SHOULD be `200`.
/// - {data} present (even if null) AND {errors} present => SHOULD be `294`
///   (partial success).
/// - {data} absent => this function does not apply; use a 4xx/5xx from the
///   request-error path instead (see `status` module).
pub fn decide_result_status(result: &GraphQLResult) -> Option<StatusDecision> {
    if !result.has_data() {
        return None;
    }
    if result.has_errors() {
        Some(StatusDecision {
            code: STATUS_PARTIAL_SUCCESS,
        })
    } else {
        Some(StatusDecision { code: 200 })
    }
}

/// A fully-formed HTTP response ready to be written by a web framework:
/// status code, `Content-Type` header value, and JSON body bytes.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Encodes a `GraphQLResult` into an `HttpResponse`, applying the spec's
/// content-negotiation-aware status/Content-Type rules.
///
/// Spec: "Body" (`#sec-Body`) - "If the `Accept` header does not indicate
/// support for one of the server's preferred media types but does indicate
/// support for `application/json` ... the request SHOULD be performed ...
/// except any response that produces a `2xx` status code should replace the
/// `Content-Type` header with `Content-Type: application/json`."
pub fn encode_response(result: &GraphQLResult, negotiated: Negotiated) -> HttpResponse {
    let status = decide_result_status(result).map(|d| d.code).unwrap_or(200);
    let body = serde_json::to_vec(result).expect("GraphQLResult serialization is infallible");

    let content_type = match negotiated {
        Negotiated::GraphqlResponseJson => {
            format!("{APPLICATION_GRAPHQL_RESPONSE_JSON}; charset=utf-8")
        }
        Negotiated::LegacyJson => {
            if (200..300).contains(&status) {
                format!("{APPLICATION_JSON}; charset=utf-8")
            } else {
                // Non-2xx legacy-negotiated responses still use the
                // spec-compliant media type per the "except 2xx" carve-out.
                format!("{APPLICATION_GRAPHQL_RESPONSE_JSON}; charset=utf-8")
            }
        }
        Negotiated::NotAcceptable => {
            // Callers should not invoke this function when negotiation
            // failed; fall back to the spec-compliant type defensively.
            format!("{APPLICATION_GRAPHQL_RESPONSE_JSON}; charset=utf-8")
        }
    };

    HttpResponse {
        status,
        content_type,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Spec: "Status Codes" (`#sec-Status-Codes`) - data present, non-null,
    /// no errors => 200.
    #[test]
    fn status_codes_sec_status_codes_data_only_yields_200() {
        let result = GraphQLResult::data_only(json!({"user": {"name": "Ada"}}));
        assert_eq!(decide_result_status(&result).unwrap().code, 200);
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) - data (even null) AND
    /// errors present => 294 partial success.
    #[test]
    fn status_codes_sec_status_codes_partial_data_and_errors_yields_294() {
        let result = GraphQLResult::partial(Value::Null, vec![json!({"message": "field error"})]);
        assert_eq!(
            decide_result_status(&result).unwrap().code,
            STATUS_PARTIAL_SUCCESS
        );

        let result_non_null = GraphQLResult::partial(
            json!({"user": null}),
            vec![json!({"message": "field error"})],
        );
        assert_eq!(
            decide_result_status(&result_non_null).unwrap().code,
            STATUS_PARTIAL_SUCCESS
        );
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) - request error result (no
    /// {data} entry) is out of scope for this decision function.
    #[test]
    fn status_codes_sec_status_codes_request_error_result_has_no_decision() {
        let result = GraphQLResult::request_error(vec![json!({"message": "bad"})]);
        assert_eq!(decide_result_status(&result), None);
    }

    /// Spec: "Body" (`#sec-Body`) - 2xx GraphQL responses negotiated for a
    /// legacy client get `Content-Type: application/json`.
    #[test]
    fn body_sec_body_legacy_negotiation_2xx_uses_application_json_content_type() {
        let result = GraphQLResult::data_only(json!({"ok": true}));
        let resp = encode_response(&result, Negotiated::LegacyJson);
        assert_eq!(resp.status, 200);
        assert!(resp.content_type.starts_with("application/json"));
    }

    /// Spec: "Body" (`#sec-Body`) - spec-compliant negotiation always uses
    /// application/graphql-response+json regardless of status.
    #[test]
    fn body_sec_body_spec_compliant_negotiation_uses_graphql_response_json() {
        let result = GraphQLResult::data_only(json!({"ok": true}));
        let resp = encode_response(&result, Negotiated::GraphqlResponseJson);
        assert!(resp
            .content_type
            .starts_with("application/graphql-response+json"));
    }

    /// Spec: "Media Types" (`#sec-Media-Types`) - responses declare UTF-8
    /// encoding explicitly (SHOULD indicate the encoding).
    #[test]
    fn media_types_sec_media_types_response_declares_charset() {
        let result = GraphQLResult::data_only(json!({"ok": true}));
        let resp = encode_response(&result, Negotiated::GraphqlResponseJson);
        assert!(resp.content_type.contains("charset=utf-8"));
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) / "Partial success"
    /// (`#sec-Partial-success`) - "If the _GraphQL result_ contains both the
    /// {data} entry (even if it is {null}) and the {errors} entry, then the
    /// server SHOULD reply with a `294` status code." Explicitly exercises
    /// the `data: null` + non-empty errors combination end-to-end through
    /// `encode_response`, verifying both the status and that the encoded
    /// body actually contains a null `data` field alongside `errors`.
    #[test]
    fn status_codes_sec_status_codes_294_partial_success_data_null_with_errors() {
        let result = GraphQLResult::partial(
            Value::Null,
            vec![json!({"message": "something went wrong"})],
        );
        assert!(result.has_data());
        assert!(!result.has_non_null_data());
        assert!(result.has_errors());

        let resp = encode_response(&result, Negotiated::GraphqlResponseJson);
        assert_eq!(resp.status, STATUS_PARTIAL_SUCCESS);

        let body: Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(body["data"], Value::Null);
        assert!(body["errors"].is_array());
        assert_eq!(body["errors"][0]["message"], "something went wrong");
    }

    /// Spec: "Body" (`#sec-Body`) - legacy mode's `Content-Type` downgrade
    /// rule is keyed on the status being in the `2xx` numeric range; the
    /// `294` partial-success status falls within `200..300`, so this
    /// crate's legacy negotiation also applies the `application/json`
    /// downgrade to it (matching the literal "any response that produces a
    /// 2xx status code" wording).
    #[test]
    fn body_sec_body_legacy_negotiation_294_in_2xx_range_downgrades_content_type() {
        let result = GraphQLResult::partial(Value::Null, vec![json!({"message": "field error"})]);
        let resp = encode_response(&result, Negotiated::LegacyJson);
        assert_eq!(resp.status, STATUS_PARTIAL_SUCCESS);
        assert!(resp.content_type.starts_with("application/json"));
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) - "If the _GraphQL result_
    /// contains the {data} entry and it is not {null}, then the server MUST
    /// reply with a `2xx` status code." and "... does not contain the
    /// {errors} entry, then the server SHOULD reply with a `200`". A
    /// data-only, non-null, error-free result always decides 200 regardless
    /// of the negotiated media type.
    #[test]
    fn status_codes_sec_status_codes_non_null_data_no_errors_is_2xx() {
        let result = GraphQLResult::data_only(json!({"user": {"name": "Ada"}}));
        assert!(result.has_non_null_data());
        assert!(!result.has_errors());
        let decision = decide_result_status(&result).unwrap();
        assert!((200..300).contains(&decision.code));
    }
}
