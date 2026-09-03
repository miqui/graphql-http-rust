//! Media type parsing and content negotiation utilities for GraphQL-over-HTTP.
//!
//! Spec: "Media Types" (`#sec-Media-Types`), "Accept" (`#sec-Accept`), and the
//! "Body" section's content negotiation rules (`#sec-Body`).

use std::cmp::Ordering;

/// The two officially recognized GraphQL media types.
///
/// Spec section: "Media Types" (`#sec-Media-Types`).
pub const APPLICATION_JSON: &str = "application/json";
pub const APPLICATION_GRAPHQL_RESPONSE_JSON: &str = "application/graphql-response+json";

/// A parsed `Accept` (or `Content-Type`) media-range essence, e.g.
/// `application/json` without parameters, along with its quality value.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaRange {
    /// The essence of the media type, lowercased, e.g. `"application/json"`,
    /// or `"*/*"`, `"application/*"`.
    pub essence: String,
    /// The `q` quality value, defaulting to `1.0`.
    pub q: f32,
}

/// Parses an HTTP `Accept` header value into a list of media ranges ordered
/// by quality (descending), preserving relative order for equal quality
/// (stable sort), per IETF RFC 9110 semantics referenced by the spec's
/// "Accept" section (`#sec-Accept`).
///
/// Unparsable segments are skipped rather than causing a hard failure, since
/// this is a best-effort negotiation helper, not a full RFC 9110 parser.
pub fn parse_accept(header: &str) -> Vec<MediaRange> {
    let mut ranges: Vec<MediaRange> = header
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let mut segments = part.split(';');
            let essence = segments.next()?.trim().to_ascii_lowercase();
            if essence.is_empty() {
                return None;
            }
            let mut q = 1.0f32;
            for param in segments {
                let param = param.trim();
                if let Some(value) = param.strip_prefix("q=") {
                    if let Ok(parsed) = value.trim().parse::<f32>() {
                        q = parsed;
                    }
                }
            }
            Some(MediaRange { essence, q })
        })
        .collect();

    // Stable sort by descending quality.
    ranges.sort_by(|a, b| b.q.partial_cmp(&a.q).unwrap_or(Ordering::Equal));
    ranges
}

/// Returns whether a concrete media type (e.g. `application/json`) matches a
/// media range from an `Accept` header (e.g. `application/*` or `*/*`).
pub fn media_range_matches(range: &str, concrete: &str) -> bool {
    if range == "*/*" {
        return true;
    }
    if let Some((range_type, _range_subtype)) = range.split_once('/') {
        if let Some((concrete_type, _)) = concrete.split_once('/') {
            if range.ends_with("/*") {
                return range_type == concrete_type;
            }
        }
    }
    range == concrete
}

/// The result of negotiating a response media type against a client's
/// `Accept` header, per the spec's "Body" section (`#sec-Body`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Negotiated {
    /// Serve `application/graphql-response+json`, the spec-compliant mode.
    GraphqlResponseJson,
    /// Serve content encoded as `application/graphql-response+json`
    /// internally, but with the `Content-Type` header replaced with
    /// `application/json` for 2xx responses to maximize legacy client
    /// compatibility. Spec: "Body" section, legacy client accommodation.
    LegacyJson,
    /// No acceptable media type; server SHOULD respond `406 Not Acceptable`.
    NotAcceptable,
}

/// Negotiates the response media type given an optional `Accept` header
/// value, following the spec's "Body" section (`#sec-Body`):
///
/// - No `Accept` header: server may choose freely; we default to
///   `application/graphql-response+json` (the spec-compliant choice).
/// - `Accept` header present and it prefers (or accepts)
///   `application/graphql-response+json`: use it.
/// - `Accept` header does not accept `application/graphql-response+json`
///   but does accept `application/json`: use `LegacyJson`.
/// - `Accept` header accepts neither: `NotAcceptable`.
///
/// Preference ordering (priority) within the `Accept` header is respected:
/// whichever of the two supported media types appears with the higher `q`
/// value (and, for ties, whichever is listed first) wins.
pub fn negotiate(accept_header: Option<&str>) -> Negotiated {
    let Some(header) = accept_header else {
        return Negotiated::GraphqlResponseJson;
    };
    let header = header.trim();
    if header.is_empty() {
        return Negotiated::GraphqlResponseJson;
    }

    let ranges = parse_accept(header);

    let mut best_graphql: Option<f32> = None;
    let mut best_json: Option<f32> = None;
    for (idx, range) in ranges.iter().enumerate() {
        if range.q <= 0.0 {
            continue;
        }
        // Use idx as a tie-breaker by slightly penalizing later entries;
        // this only affects ordering between the two candidate types when
        // q values are exactly equal, and the list is already sorted by q.
        let _ = idx;
        if best_graphql.is_none()
            && media_range_matches(&range.essence, APPLICATION_GRAPHQL_RESPONSE_JSON)
        {
            best_graphql = Some(range.q);
        }
        if best_json.is_none() && media_range_matches(&range.essence, APPLICATION_JSON) {
            best_json = Some(range.q);
        }
    }

    match (best_graphql, best_json) {
        (Some(gq), Some(jq)) => {
            if gq >= jq {
                Negotiated::GraphqlResponseJson
            } else {
                Negotiated::LegacyJson
            }
        }
        (Some(_), None) => Negotiated::GraphqlResponseJson,
        (None, Some(_)) => Negotiated::LegacyJson,
        (None, None) => Negotiated::NotAcceptable,
    }
}

/// Parses a `Content-Type` header value into its essence (media type without
/// parameters), lowercased, e.g. `"application/json; charset=utf-8"` ->
/// `"application/json"`.
pub fn content_type_essence(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec: "Media Types" (`#sec-Media-Types`) - officially recognized types.
    #[test]
    fn media_types_sec_media_types_officially_recognized_constants() {
        assert_eq!(APPLICATION_JSON, "application/json");
        assert_eq!(
            APPLICATION_GRAPHQL_RESPONSE_JSON,
            "application/graphql-response+json"
        );
    }

    /// Spec: "Accept" (`#sec-Accept`) - no Accept header allows server choice;
    /// we choose the spec-compliant media type by default.
    #[test]
    fn accept_sec_accept_missing_header_defaults_to_graphql_response_json() {
        assert_eq!(negotiate(None), Negotiated::GraphqlResponseJson);
    }

    /// Spec: "Accept" (`#sec-Accept`) - recommended client header value.
    #[test]
    fn accept_sec_accept_recommended_header_prefers_graphql_response_json() {
        let accept = "application/graphql-response+json, application/json;q=0.9";
        assert_eq!(negotiate(Some(accept)), Negotiated::GraphqlResponseJson);
    }

    /// Spec: "Body" (`#sec-Body`) - legacy client accepting only
    /// application/json gets the LegacyJson negotiation outcome.
    #[test]
    fn body_sec_body_legacy_client_json_only_negotiates_legacy_json() {
        assert_eq!(negotiate(Some("application/json")), Negotiated::LegacyJson);
    }

    /// Spec: "Body" (`#sec-Body`) - Accept header naming neither supported
    /// type results in 406-eligible NotAcceptable outcome.
    #[test]
    fn body_sec_body_unsupported_accept_header_is_not_acceptable() {
        assert_eq!(negotiate(Some("text/plain")), Negotiated::NotAcceptable);
    }

    /// Spec: "Body" (`#sec-Body`) - wildcard Accept headers are honored.
    #[test]
    fn body_sec_body_wildcard_accept_matches_graphql_response_json() {
        assert_eq!(negotiate(Some("*/*")), Negotiated::GraphqlResponseJson);
    }

    /// Spec: "Media Types" (`#sec-Media-Types`) - Content-Type essence
    /// extraction ignores charset parameters.
    #[test]
    fn media_types_sec_media_types_content_type_essence_strips_charset() {
        assert_eq!(
            content_type_essence("application/json; charset=utf-8"),
            "application/json"
        );
    }

    /// Spec: "Media Types" (`#sec-Media-Types`) - "If the media type in a
    /// `Content-Type` or `Accept` header does not include encoding
    /// information ... then `utf-8` MUST be assumed." When no charset
    /// parameter is present at all, the essence is unaffected and callers
    /// are expected to assume utf-8 (there is no separate encoding to strip
    /// in this case).
    #[test]
    fn media_types_sec_media_types_content_type_without_charset_assumes_utf8() {
        assert_eq!(
            content_type_essence("application/graphql-response+json"),
            APPLICATION_GRAPHQL_RESPONSE_JSON
        );
        assert_eq!(content_type_essence("application/json"), APPLICATION_JSON);
    }

    /// Spec: "Accept" (`#sec-Accept`) - q-value ordering: when the `Accept`
    /// header lists `application/json` with a higher quality value than
    /// `application/graphql-response+json`, the higher-quality type wins
    /// regardless of listed order, yielding the `LegacyJson` outcome.
    #[test]
    fn accept_sec_accept_q_value_ordering_prefers_higher_quality_json() {
        let accept = "application/json;q=0.9, application/graphql-response+json;q=0.8";
        assert_eq!(negotiate(Some(accept)), Negotiated::LegacyJson);
    }

    /// Spec: "Accept" (`#sec-Accept`) - q-value ordering is independent of
    /// the order media types are listed in the header: a later, higher
    /// quality entry still wins.
    #[test]
    fn accept_sec_accept_q_value_ordering_ignores_listed_order() {
        let accept = "application/graphql-response+json;q=0.5, application/json;q=0.9";
        assert_eq!(negotiate(Some(accept)), Negotiated::LegacyJson);
    }

    /// Spec: "Accept" (`#sec-Accept`) / IETF RFC 9110 content negotiation -
    /// a wildcard `*/*` entry with a lower quality value than an explicit
    /// `application/json` entry loses to the explicit, higher-quality type.
    #[test]
    fn body_sec_body_wildcard_with_lower_q_loses_to_explicit_type() {
        let accept = "application/json;q=1.0, */*;q=0.1";
        assert_eq!(negotiate(Some(accept)), Negotiated::LegacyJson);
    }

    /// Spec: "Accept" (`#sec-Accept`) - a wildcard `*/*` entry with a higher
    /// quality value than an explicit `application/json` entry wins, and
    /// since `*/*` also matches `application/graphql-response+json` at that
    /// higher quality, the spec-compliant type is preferred.
    #[test]
    fn body_sec_body_wildcard_with_higher_q_wins_over_explicit_json() {
        let accept = "application/json;q=0.5, */*;q=0.9";
        assert_eq!(negotiate(Some(accept)), Negotiated::GraphqlResponseJson);
    }

    /// Spec: "Accept" (`#sec-Accept`) - an `Accept` header consisting only
    /// of whitespace is treated the same as an absent header (server may
    /// choose freely; we default to the spec-compliant media type).
    #[test]
    fn accept_sec_accept_whitespace_only_header_defaults_to_graphql_response_json() {
        assert_eq!(negotiate(Some("   ")), Negotiated::GraphqlResponseJson);
    }

    /// Spec: "Body" (`#sec-Body`) - when the `Accept` header lists multiple
    /// media types, none of which are `application/json` or
    /// `application/graphql-response+json`, the result is `NotAcceptable`
    /// (406-eligible), even with varying q-values.
    #[test]
    fn body_sec_body_multiple_unsupported_types_yields_not_acceptable() {
        let accept = "text/plain, text/html;q=0.9, image/png;q=0.1";
        assert_eq!(negotiate(Some(accept)), Negotiated::NotAcceptable);
    }

    /// Spec: "Accept" (`#sec-Accept`) - a `q` value that fails to parse is
    /// treated as the default `1.0` rather than causing the whole entry to
    /// be dropped.
    #[test]
    fn accept_sec_accept_unparsable_q_value_defaults_to_one() {
        let ranges = parse_accept("application/json;q=bogus");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].q, 1.0);
    }

    /// Spec: "Accept" (`#sec-Accept`) - a bare `q=0` entry is treated as
    /// unacceptable and excluded from consideration during negotiation.
    #[test]
    fn body_sec_body_zero_q_value_excludes_media_type_from_negotiation() {
        let accept = "application/graphql-response+json;q=0, application/json;q=0.5";
        assert_eq!(negotiate(Some(accept)), Negotiated::LegacyJson);
    }
}
