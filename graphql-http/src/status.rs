//! HTTP status-code decision helpers for request-level (pre-execution)
//! failures.
//!
//! Spec: "Status Codes" (`#sec-Status-Codes`) - the bullet list of `4xx`/`5xx`
//! codes for specific failure conditions.

/// Enumerates the request-level failure conditions called out by the spec's
/// "Status Codes" section (`#sec-Status-Codes`), each mapped to its
/// RECOMMENDED status code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestFailure {
    /// Mutation attempted via GET. RECOMMENDED: 405.
    MutationViaGet,
    /// Unsupported HTTP method used. RECOMMENDED: 405.
    UnsupportedMethod,
    /// Unsupported request `Content-Type`. RECOMMENDED: 415.
    UnsupportedContentType,
    /// No supported media type in `Accept`, and `application/json` not
    /// included either. RECOMMENDED: 406.
    NotAcceptable,
    /// Missing `Content-Type` header on a POST request. SHOULD reject with
    /// an appropriate 4xx; we recommend 400 as a sensible default.
    MissingContentType,
    /// Client took too long to produce a request. RECOMMENDED: 408.
    RequestTimeout,
    /// URI too large. RECOMMENDED: 414.
    UriTooLarge,
    /// Request headers too large. RECOMMENDED: 431.
    HeadersTooLarge,
    /// POST body too large. RECOMMENDED: 413.
    BodyTooLarge,
    /// JSON body could not be parsed. RECOMMENDED: 400.
    JsonParseFailure,
    /// Not a well-formed GraphQL-over-HTTP request. RECOMMENDED: 422.
    NotWellFormedRequest,
    /// GraphQL document could not be parsed. RECOMMENDED: 400.
    DocumentParseFailure,
    /// Request does not pass GraphQL validation. RECOMMENDED: 422.
    ValidationFailure,
    /// Operation to execute cannot be unambiguously determined. RECOMMENDED: 422.
    AmbiguousOperation,
    /// Variable coercion failure. RECOMMENDED: 422.
    VariableCoercionFailure,
    /// Client not permitted to issue the request. RECOMMENDED: 401/403 (we
    /// default to 403; callers may override).
    Forbidden,
    /// Server cannot process the request for maintenance/load-shedding
    /// reasons. RECOMMENDED: 503.
    ServiceUnavailable,
}

impl RequestFailure {
    /// The RECOMMENDED HTTP status code for this failure condition, per
    /// "Status Codes" (`#sec-Status-Codes`).
    pub fn recommended_status(self) -> u16 {
        match self {
            RequestFailure::MutationViaGet => 405,
            RequestFailure::UnsupportedMethod => 405,
            RequestFailure::UnsupportedContentType => 415,
            RequestFailure::NotAcceptable => 406,
            RequestFailure::MissingContentType => 400,
            RequestFailure::RequestTimeout => 408,
            RequestFailure::UriTooLarge => 414,
            RequestFailure::HeadersTooLarge => 431,
            RequestFailure::BodyTooLarge => 413,
            RequestFailure::JsonParseFailure => 400,
            RequestFailure::NotWellFormedRequest => 422,
            RequestFailure::DocumentParseFailure => 400,
            RequestFailure::ValidationFailure => 422,
            RequestFailure::AmbiguousOperation => 422,
            RequestFailure::VariableCoercionFailure => 422,
            RequestFailure::Forbidden => 403,
            RequestFailure::ServiceUnavailable => 503,
        }
    }

    /// Whether this failure, per RFC 9110 semantics referenced by the spec,
    /// requires an `Allow` header to accompany the response (true for `405`
    /// responses).
    ///
    /// Spec: "GET" note - "If status code `405` is used then the `Allow`
    /// header must be included as required by IETF RFC 9110"; and the final
    /// note under "Status Codes" repeating the same requirement.
    pub fn requires_allow_header(self) -> bool {
        self.recommended_status() == 405
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec: "GET" (`#sec-GET`) - mutation via GET MUST be a 4xx; 405
    /// RECOMMENDED.
    #[test]
    fn get_sec_get_mutation_via_get_recommends_405() {
        assert_eq!(RequestFailure::MutationViaGet.recommended_status(), 405);
        assert!(RequestFailure::MutationViaGet.requires_allow_header());
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) - unsupported Content-Type
    /// recommends 415.
    #[test]
    fn status_codes_sec_status_codes_unsupported_content_type_recommends_415() {
        assert_eq!(
            RequestFailure::UnsupportedContentType.recommended_status(),
            415
        );
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) - unacceptable Accept
    /// header (no application/json fallback) recommends 406.
    #[test]
    fn status_codes_sec_status_codes_not_acceptable_recommends_406() {
        assert_eq!(RequestFailure::NotAcceptable.recommended_status(), 406);
    }

    /// Spec: "JSON parsing failure" example (`#sec-Status-Codes`) - JSON
    /// parse failure recommends 400.
    #[test]
    fn status_codes_sec_status_codes_json_parse_failure_recommends_400() {
        assert_eq!(RequestFailure::JsonParseFailure.recommended_status(), 400);
    }

    /// Spec: "Invalid parameters" example (`#sec-Status-Codes`) - not a
    /// well-formed request recommends 422.
    #[test]
    fn status_codes_sec_status_codes_not_well_formed_recommends_422() {
        assert_eq!(
            RequestFailure::NotWellFormedRequest.recommended_status(),
            422
        );
    }

    /// Spec: "Document parsing failure" example (`#sec-Status-Codes`) -
    /// unparsable GraphQL document recommends 400.
    #[test]
    fn status_codes_sec_status_codes_document_parse_failure_recommends_400() {
        assert_eq!(
            RequestFailure::DocumentParseFailure.recommended_status(),
            400
        );
    }

    /// Spec: "Document validation failure" example (`#sec-Status-Codes`) -
    /// validation failure recommends 422.
    #[test]
    fn status_codes_sec_status_codes_validation_failure_recommends_422() {
        assert_eq!(RequestFailure::ValidationFailure.recommended_status(), 422);
    }

    /// Spec: "Operation cannot be determined" example (`#sec-Status-Codes`)
    /// - ambiguous operation recommends 422.
    #[test]
    fn status_codes_sec_status_codes_ambiguous_operation_recommends_422() {
        assert_eq!(RequestFailure::AmbiguousOperation.recommended_status(), 422);
    }

    /// Spec: "Variable coercion failure" example (`#sec-Status-Codes`) -
    /// coercion failure recommends 422.
    #[test]
    fn status_codes_sec_status_codes_variable_coercion_failure_recommends_422() {
        assert_eq!(
            RequestFailure::VariableCoercionFailure.recommended_status(),
            422
        );
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) - server-side
    /// maintenance/load-shedding recommends 503.
    #[test]
    fn status_codes_sec_status_codes_service_unavailable_recommends_503() {
        assert_eq!(RequestFailure::ServiceUnavailable.recommended_status(), 503);
    }

    /// Spec: "Status Codes" (`#sec-Status-Codes`) - only 405 responses
    /// require the Allow header per RFC 9110 cross-reference.
    #[test]
    fn status_codes_sec_status_codes_only_405_requires_allow_header() {
        assert!(RequestFailure::UnsupportedMethod.requires_allow_header());
        assert!(!RequestFailure::NotAcceptable.requires_allow_header());
        assert!(!RequestFailure::JsonParseFailure.requires_allow_header());
    }
}
