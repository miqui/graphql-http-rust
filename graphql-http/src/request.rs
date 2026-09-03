//! GraphQL-over-HTTP request representation and parsing.
//!
//! Spec: "Request" (`#sec-Request`), "GraphQL-over-HTTP Request"
//! (`#sec-GraphQL-over-HTTP-Request`), "GET" (`#sec-GET`), "POST"
//! (`#sec-POST`), and "JSON Encoding" (`#sec-JSON-Encoding`).

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// A parsed, well-formed _GraphQL-over-HTTP request_.
///
/// Spec: "GraphQL-over-HTTP Request" (`#sec-GraphQL-over-HTTP-Request`).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphQLRequest {
    /// {query} - Required string: source text of a GraphQL Document.
    pub query: String,
    /// {operationName} - Optional string.
    pub operation_name: Option<String>,
    /// {variables} - Optional map.
    pub variables: Option<serde_json::Map<String, Value>>,
    /// {extensions} - Optional map.
    pub extensions: Option<serde_json::Map<String, Value>>,
}

/// Errors that can occur while parsing a request into a well-formed
/// _GraphQL-over-HTTP request_.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RequestParseError {
    /// The request body/query string could not be parsed at all (e.g.
    /// invalid JSON). Spec: "JSON parsing failure" example (`#sec-Status-Codes`),
    /// recommends `400`.
    #[error("body could not be parsed: {0}")]
    NotParsable(String),

    /// The request was parsed but does not constitute a well-formed
    /// _GraphQL-over-HTTP request_ (missing/wrong-typed required
    /// parameters, or malformed sub-encoded values in GET params). Spec:
    /// "Invalid parameters" example (`#sec-Status-Codes`), recommends `422`.
    #[error("not a well-formed GraphQL-over-HTTP request: {0}")]
    NotWellFormed(String),

    /// A mutation was attempted via GET. Spec: "GET" (`#sec-GET`), MUST
    /// respond with 4xx, 405 RECOMMENDED.
    #[error("mutation operations must not be executed via GET")]
    MutationViaGet,
}

/// JSON shape used to deserialize a POST body per "JSON Encoding"
/// (`#sec-JSON-Encoding`). All fields optional at the JSON level so we can
/// produce precise well-formedness errors ourselves.
#[derive(Debug, Deserialize)]
struct RawJsonRequest {
    #[serde(default)]
    query: Option<Value>,
    #[serde(default, rename = "operationName")]
    operation_name: Option<Value>,
    #[serde(default)]
    variables: Option<Value>,
    #[serde(default)]
    extensions: Option<Value>,
}

/// Parses a JSON-encoded POST body into a well-formed _GraphQL-over-HTTP
/// request_.
///
/// Spec: "JSON Encoding" (`#sec-JSON-Encoding`) - {query} required string;
/// {operationName} optional string; {variables} and {extensions} optional
/// objects; `null` for optional parameters is equivalent to omission;
/// unrecognized properties MUST be ignored (handled implicitly since we
/// deserialize into a struct with only the known fields).
pub fn parse_json_body(body: &[u8]) -> Result<GraphQLRequest, RequestParseError> {
    let raw: RawJsonRequest =
        serde_json::from_slice(body).map_err(|e| RequestParseError::NotParsable(e.to_string()))?;

    let query = match raw.query {
        Some(Value::String(s)) => s,
        Some(_) => {
            return Err(RequestParseError::NotWellFormed(
                "query must be a string".into(),
            ))
        }
        None => return Err(RequestParseError::NotWellFormed("query is required".into())),
    };

    let operation_name = match raw.operation_name {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s),
        Some(_) => {
            return Err(RequestParseError::NotWellFormed(
                "operationName must be a string".into(),
            ))
        }
    };

    let variables = match raw.variables {
        None | Some(Value::Null) => None,
        Some(Value::Object(map)) => Some(map),
        Some(_) => {
            return Err(RequestParseError::NotWellFormed(
                "variables must be a map".into(),
            ))
        }
    };

    let extensions = match raw.extensions {
        None | Some(Value::Null) => None,
        Some(Value::Object(map)) => Some(map),
        Some(_) => {
            return Err(RequestParseError::NotWellFormed(
                "extensions must be a map".into(),
            ))
        }
    };

    Ok(GraphQLRequest {
        query,
        operation_name,
        variables,
        extensions,
    })
}

/// Parses `application/x-www-form-urlencoded` query-string parameters (as
/// produced by decoding a GET request's query component) into a well-formed
/// _GraphQL-over-HTTP request_.
///
/// Spec: "GET" (`#sec-GET`) - parameters MUST be provided in the query
/// component encoded as `application/x-www-form-urlencoded`; {variables}
/// and {extensions}, if present and non-empty, MUST be JSON-encoded strings;
/// empty string for optional parameters is equivalent to omission.
///
/// `params` should already be decoded key/value pairs (this crate does not
/// perform URL-decoding itself; callers use their HTTP framework's query
/// parser, e.g. `serde_urlencoded` or the web framework's own facility).
pub fn parse_get_params(
    params: &HashMap<String, String>,
) -> Result<GraphQLRequest, RequestParseError> {
    let query = match params.get("query") {
        Some(q) if !q.is_empty() => q.clone(),
        _ => return Err(RequestParseError::NotWellFormed("query is required".into())),
    };

    let operation_name = match params.get("operationName") {
        Some(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };

    let variables = match params.get("variables") {
        Some(s) if !s.is_empty() => {
            let value: Value = serde_json::from_str(s).map_err(|e| {
                RequestParseError::NotWellFormed(format!("invalid variables JSON: {e}"))
            })?;
            match value {
                Value::Object(map) => Some(map),
                _ => {
                    return Err(RequestParseError::NotWellFormed(
                        "variables must be a JSON object".into(),
                    ))
                }
            }
        }
        _ => None,
    };

    let extensions = match params.get("extensions") {
        Some(s) if !s.is_empty() => {
            let value: Value = serde_json::from_str(s).map_err(|e| {
                RequestParseError::NotWellFormed(format!("invalid extensions JSON: {e}"))
            })?;
            match value {
                Value::Object(map) => Some(map),
                _ => {
                    return Err(RequestParseError::NotWellFormed(
                        "extensions must be a JSON object".into(),
                    ))
                }
            }
        }
        _ => None,
    };

    Ok(GraphQLRequest {
        query,
        operation_name,
        variables,
        extensions,
    })
}

/// Naive heuristic to detect whether a GraphQL document's *anonymous* or
/// selected operation is a mutation, for the purpose of enforcing the
/// spec's GET-must-not-execute-mutations rule at the HTTP layer.
///
/// Spec: "GET" (`#sec-GET`) - "GET requests MUST NOT be used for executing
/// mutation operations."
///
/// Note: this is intentionally simplistic (string scanning, not a full
/// GraphQL parser) since full document parsing is out of scope for the
/// HTTP-layer crate; real integrations should perform this check using
/// their GraphQL execution engine's parsed AST when available, and can
/// bypass/override this helper.
pub fn document_looks_like_mutation(query: &str, operation_name: Option<&str>) -> bool {
    // Strip comments and find `mutation` operation definitions naively.
    let lowered = query;
    let mut looks_like_mutation_names: Vec<Option<String>> = Vec::new();

    let bytes = lowered.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if lowered[i..].starts_with("mutation") {
            // Ensure it's a keyword boundary (not part of an identifier).
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_idx = i + "mutation".len();
            let after_ok = after_idx >= bytes.len() || !is_ident_byte(bytes[after_idx]);
            if before_ok && after_ok {
                // Parse optional operation name that follows.
                let rest = &lowered[after_idx..];
                let trimmed = rest.trim_start();
                let name: Option<String> = {
                    let mut chars = trimmed.chars();
                    let mut name = String::new();
                    for c in chars.by_ref() {
                        if c.is_alphanumeric() || c == '_' {
                            name.push(c);
                        } else {
                            break;
                        }
                    }
                    if name.is_empty() {
                        None
                    } else {
                        Some(name)
                    }
                };
                looks_like_mutation_names.push(name);
            }
        }
        i += 1;
    }

    if looks_like_mutation_names.is_empty() {
        return false;
    }

    match operation_name {
        Some(name) => looks_like_mutation_names
            .iter()
            .any(|n| n.as_deref() == Some(name)),
        // If there's exactly one operation in the document and it's a
        // mutation, treat it as the anonymous operation being executed.
        None => looks_like_mutation_names.len() == 1,
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Spec: "JSON Encoding" (`#sec-JSON-Encoding`) - query is required.
    #[test]
    fn json_encoding_sec_json_encoding_query_required() {
        let err = parse_json_body(br#"{"operationName":"Q"}"#).unwrap_err();
        assert_eq!(
            err,
            RequestParseError::NotWellFormed("query is required".into())
        );
    }

    /// Spec: "JSON Encoding" (`#sec-JSON-Encoding`) - well-formed request
    /// with all four parameters parses correctly.
    #[test]
    fn json_encoding_sec_json_encoding_parses_all_parameters() {
        let body = json!({
            "query": "query ($id: ID!) { user(id: $id) { name } }",
            "operationName": "Q",
            "variables": {"id": "abc"},
            "extensions": {"trace": true}
        })
        .to_string();
        let parsed = parse_json_body(body.as_bytes()).unwrap();
        assert_eq!(parsed.query, "query ($id: ID!) { user(id: $id) { name } }");
        assert_eq!(parsed.operation_name.as_deref(), Some("Q"));
        assert_eq!(parsed.variables.unwrap().get("id").unwrap(), "abc");
        assert_eq!(
            parsed.extensions.unwrap().get("trace").unwrap(),
            &Value::Bool(true)
        );
    }

    /// Spec: "JSON Encoding" (`#sec-JSON-Encoding`) - `null` for optional
    /// parameters is equivalent to omission.
    #[test]
    fn json_encoding_sec_json_encoding_null_optional_params_equivalent_to_omission() {
        let body = json!({
            "query": "{ __typename }",
            "operationName": null,
            "variables": null,
            "extensions": null
        })
        .to_string();
        let parsed = parse_json_body(body.as_bytes()).unwrap();
        assert_eq!(parsed.operation_name, None);
        assert_eq!(parsed.variables, None);
        assert_eq!(parsed.extensions, None);
    }

    /// Spec: "JSON parsing failure" example (`#sec-Status-Codes`) - invalid
    /// JSON body cannot be parsed at all.
    #[test]
    fn status_codes_sec_status_codes_invalid_json_yields_not_parsable() {
        let err = parse_json_body(b"NONSENSE").unwrap_err();
        assert!(matches!(err, RequestParseError::NotParsable(_)));

        let err2 = parse_json_body(br#"{"query":"#).unwrap_err();
        assert!(matches!(err2, RequestParseError::NotParsable(_)));
    }

    /// Spec: "Invalid parameters" example (`#sec-Status-Codes`) - wrong
    /// shape for variables is not well-formed.
    #[test]
    fn status_codes_sec_status_codes_invalid_variables_shape_not_well_formed() {
        let body = json!({"query": "query Q ($i:Int!) { q(i: $i) }", "variables": [7]}).to_string();
        let err = parse_json_body(body.as_bytes()).unwrap_err();
        assert!(matches!(err, RequestParseError::NotWellFormed(_)));
    }

    /// Spec: "GET" (`#sec-GET`) - query, variables and extensions are
    /// decoded from x-www-form-urlencoded query params.
    #[test]
    fn get_sec_get_parses_query_variables_and_extensions() {
        let mut params = HashMap::new();
        params.insert(
            "query".to_string(),
            "query($id: ID!){user(id:$id){name}}".to_string(),
        );
        params.insert(
            "variables".to_string(),
            r#"{"id":"QVBJcy5ndXJ1"}"#.to_string(),
        );
        let parsed = parse_get_params(&params).unwrap();
        assert_eq!(parsed.query, "query($id: ID!){user(id:$id){name}}");
        assert_eq!(parsed.variables.unwrap().get("id").unwrap(), "QVBJcy5ndXJ1");
    }

    /// Spec: "GET" (`#sec-GET`) - empty string for optional parameters is
    /// equivalent to not specifying them.
    #[test]
    fn get_sec_get_empty_string_optional_params_equivalent_to_omission() {
        let mut params = HashMap::new();
        params.insert("query".to_string(), "{ __typename }".to_string());
        params.insert("operationName".to_string(), "".to_string());
        params.insert("variables".to_string(), "".to_string());
        let parsed = parse_get_params(&params).unwrap();
        assert_eq!(parsed.operation_name, None);
        assert_eq!(parsed.variables, None);
    }

    /// Spec: "GET" (`#sec-GET`) - missing query is not well-formed.
    #[test]
    fn get_sec_get_missing_query_not_well_formed() {
        let params = HashMap::new();
        let err = parse_get_params(&params).unwrap_err();
        assert!(matches!(err, RequestParseError::NotWellFormed(_)));
    }

    /// Spec: "GET" (`#sec-GET`) - MUST NOT execute mutation operations;
    /// this test validates the detection helper used to enforce that.
    #[test]
    fn get_sec_get_detects_anonymous_mutation_operation() {
        assert!(document_looks_like_mutation(
            "mutation { createThing { id } }",
            None
        ));
        assert!(!document_looks_like_mutation(
            "query { thing { id } }",
            None
        ));
    }

    /// Spec: "GET" (`#sec-GET`) - detects named mutation operation matching
    /// the requested operationName.
    #[test]
    fn get_sec_get_detects_named_mutation_operation() {
        let doc = "mutation DoIt { createThing { id } } query Q { thing { id } }";
        assert!(document_looks_like_mutation(doc, Some("DoIt")));
        assert!(!document_looks_like_mutation(doc, Some("Q")));
    }

    /// Spec: "JSON Encoding" (`#sec-JSON-Encoding`) - "Servers receiving a
    /// request with additional properties MUST ignore properties they do
    /// not understand." Unknown/extra top-level properties in the JSON body
    /// must be silently ignored rather than causing a parse failure.
    #[test]
    fn json_encoding_sec_json_encoding_unknown_properties_are_ignored() {
        let body = json!({
            "query": "{ __typename }",
            "unknownProperty": "should be ignored",
            "anotherOne": {"nested": [1, 2, 3]},
            "yetAnother": null
        })
        .to_string();
        let parsed = parse_json_body(body.as_bytes()).unwrap();
        assert_eq!(parsed.query, "{ __typename }");
        assert_eq!(parsed.operation_name, None);
    }

    /// Spec: "Invalid parameters" example (`#sec-Status-Codes`) - a POST
    /// body of `{"qeury": "{__typename}"}` (typo'd property name, missing
    /// the required `query`) is not a well-formed request and SHOULD result
    /// in status code 422.
    #[test]
    fn status_codes_sec_status_codes_typo_qeury_property_not_well_formed() {
        let err = parse_json_body(br#"{"qeury": "{__typename}"}"#).unwrap_err();
        assert_eq!(
            err,
            RequestParseError::NotWellFormed("query is required".into())
        );
    }

    /// Spec: "Invalid parameters" example (`#sec-Status-Codes`) - exact
    /// spec example: `{"query": "query Q ($i:Int!) { q(i: $i) }",
    /// "variables": [7]}` (invalid shape for `variables`, an array instead
    /// of a map) is not well-formed.
    #[test]
    fn status_codes_sec_status_codes_variables_array_shape_not_well_formed() {
        let body = r#"{"query": "query Q ($i:Int!) { q(i: $i) }", "variables": [7]}"#;
        let err = parse_json_body(body.as_bytes()).unwrap_err();
        assert_eq!(
            err,
            RequestParseError::NotWellFormed("variables must be a map".into())
        );
    }

    /// Spec: "JSON parsing failure" example (`#sec-Status-Codes`) - the
    /// exact spec examples `NONSENSE` and `{"query":` (invalid JSON) must
    /// not parse, and are distinct from a well-formed-but-invalid request.
    #[test]
    fn status_codes_sec_status_codes_malformed_json_body_not_parsable() {
        assert!(matches!(
            parse_json_body(b"NONSENSE"),
            Err(RequestParseError::NotParsable(_))
        ));
        assert!(matches!(
            parse_json_body(br#"{"query":"#),
            Err(RequestParseError::NotParsable(_))
        ));
    }

    /// Spec: "JSON Encoding" (`#sec-JSON-Encoding`) - `query` present but of
    /// the wrong type (not a string) is not well-formed, distinct from the
    /// JSON-parse-failure case.
    #[test]
    fn json_encoding_sec_json_encoding_query_wrong_type_not_well_formed() {
        let body = json!({"query": 123}).to_string();
        let err = parse_json_body(body.as_bytes()).unwrap_err();
        assert_eq!(
            err,
            RequestParseError::NotWellFormed("query must be a string".into())
        );
    }
}
