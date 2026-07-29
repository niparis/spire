//! Signed-ingress admission rules for the public Linear webhook boundary.
//!
//! Everything in this module is pure so that the HTTP adapter only has to read
//! the raw body once and hand it here. The boundary performs no Linear API call
//! and starts no work: it decides whether a delivery may be stored durably.

use std::collections::BTreeSet;

use hmac::{Hmac, Mac};
use serde::Serialize;
use serde_json::Value;
use sha2::Sha256;

/// Header Linear sets on every delivery. It is the durable de-duplication key.
pub const DELIVERY_HEADER: &str = "linear-delivery";
pub const SIGNATURE_HEADER: &str = "linear-signature";
pub const EVENT_HEADER: &str = "linear-event";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebhookLimits {
    pub max_body_bytes: usize,
    pub replay_window_seconds: u64,
}

/// Identity the delivery must claim before it is stored. Team-level and
/// work-type allowlisting belong to the rollout gate, not to this boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebhookAllowlist<'a> {
    pub organization_id: &'a str,
    pub webhook_id: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct WebhookRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub content_type: Option<&'a str>,
    pub signature: Option<&'a str>,
    pub delivery_id: Option<&'a str>,
    pub declared_length: Option<usize>,
    pub body: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookRejection {
    MethodNotAllowed,
    PathNotFound,
    UnsupportedMediaType,
    BodyTooLarge,
    MissingSignature,
    InvalidSignature,
    StaleTimestamp,
    MalformedPayload,
    UnknownOrganization,
    UnknownWebhook,
}

impl WebhookRejection {
    /// Stable, non-sensitive reason string for logs and metrics.
    pub fn reason(self) -> &'static str {
        match self {
            Self::MethodNotAllowed => "method_not_allowed",
            Self::PathNotFound => "path_not_found",
            Self::UnsupportedMediaType => "unsupported_media_type",
            Self::BodyTooLarge => "body_too_large",
            Self::MissingSignature => "missing_signature",
            Self::InvalidSignature => "invalid_signature",
            Self::StaleTimestamp => "stale_timestamp",
            Self::MalformedPayload => "malformed_payload",
            Self::UnknownOrganization => "unknown_organization",
            Self::UnknownWebhook => "unknown_webhook",
        }
    }

    /// Signature, replay, and identity failures are authentication failures.
    /// Everything else is a client error the sender can see and correct.
    pub fn is_authentication_failure(self) -> bool {
        matches!(
            self,
            Self::MissingSignature
                | Self::InvalidSignature
                | Self::StaleTimestamp
                | Self::UnknownOrganization
                | Self::UnknownWebhook
        )
    }
}

/// Normalized Linear delivery envelope. Issue text never reaches this type: the
/// raw body stays in the inbox for evidence and the canonical issue is fetched
/// separately during processing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinearWebhookEnvelope {
    pub delivery_id: String,
    pub event_type: String,
    pub action: String,
    pub organization_id: String,
    pub webhook_id: String,
    pub webhook_timestamp_millis: i64,
    pub issue_id: Option<String>,
    pub team_id: Option<String>,
    pub actor_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AcceptedDelivery {
    pub envelope: LinearWebhookEnvelope,
    /// Selected headers, with the signature deliberately omitted.
    pub redacted_headers: Value,
}

/// Verifies HMAC-SHA256 over the exact raw body using a constant-time compare.
pub fn signature_matches(secret: &[u8], body: &[u8], header: &str) -> bool {
    let Ok(provided) = hex::decode(header.trim()) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&provided).is_ok()
}

/// Applies every ingress rule in order and returns the row the adapter must
/// commit before it answers `200`.
pub fn accept_delivery(
    request: &WebhookRequest<'_>,
    configured_path: &str,
    limits: WebhookLimits,
    secret: &[u8],
    allowlist: WebhookAllowlist<'_>,
    now_unix_seconds: i64,
) -> Result<AcceptedDelivery, WebhookRejection> {
    if !request.method.eq_ignore_ascii_case("POST") {
        return Err(WebhookRejection::MethodNotAllowed);
    }
    if request.path != configured_path {
        return Err(WebhookRejection::PathNotFound);
    }
    if !request.content_type.is_some_and(|value| {
        value
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("application/json")
    }) {
        return Err(WebhookRejection::UnsupportedMediaType);
    }
    if request
        .declared_length
        .is_some_and(|length| length > limits.max_body_bytes)
        || request.body.len() > limits.max_body_bytes
    {
        return Err(WebhookRejection::BodyTooLarge);
    }
    let Some(signature) = request.signature else {
        return Err(WebhookRejection::MissingSignature);
    };
    if !signature_matches(secret, request.body, signature) {
        return Err(WebhookRejection::InvalidSignature);
    }

    let payload: Value =
        serde_json::from_slice(request.body).map_err(|_| WebhookRejection::MalformedPayload)?;
    let envelope = normalize_envelope(&payload, request.delivery_id, request.body)?;
    if envelope.organization_id != allowlist.organization_id {
        return Err(WebhookRejection::UnknownOrganization);
    }
    if envelope.webhook_id != allowlist.webhook_id {
        return Err(WebhookRejection::UnknownWebhook);
    }
    if !within_replay_window(
        envelope.webhook_timestamp_millis,
        now_unix_seconds,
        limits.replay_window_seconds,
    ) {
        return Err(WebhookRejection::StaleTimestamp);
    }

    let redacted_headers = serde_json::json!({
        DELIVERY_HEADER: envelope.delivery_id,
        EVENT_HEADER: envelope.event_type,
        SIGNATURE_HEADER: "[redacted]",
        "content-type": request.content_type,
    });
    Ok(AcceptedDelivery {
        envelope,
        redacted_headers,
    })
}

fn within_replay_window(timestamp_millis: i64, now_unix_seconds: i64, window_seconds: u64) -> bool {
    let window_millis = i64::try_from(window_seconds.saturating_mul(1_000)).unwrap_or(i64::MAX);
    let now_millis = now_unix_seconds.saturating_mul(1_000);
    now_millis.saturating_sub(timestamp_millis).abs() <= window_millis
}

fn normalize_envelope(
    payload: &Value,
    delivery_header: Option<&str>,
    body: &[u8],
) -> Result<LinearWebhookEnvelope, WebhookRejection> {
    let string = |pointer: &str| {
        payload
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let event_type = string("/type").ok_or(WebhookRejection::MalformedPayload)?;
    let action = string("/action").ok_or(WebhookRejection::MalformedPayload)?;
    let organization_id = string("/organizationId").ok_or(WebhookRejection::MalformedPayload)?;
    let webhook_id = string("/webhookId").ok_or(WebhookRejection::MalformedPayload)?;
    let webhook_timestamp_millis = payload
        .pointer("/webhookTimestamp")
        .and_then(Value::as_i64)
        .ok_or(WebhookRejection::MalformedPayload)?;
    // Linear resources reference the issue differently: an `Issue` event carries
    // it directly, a `Comment` event nests it, and a label event may not name one.
    let issue_id = match event_type.as_str() {
        "Issue" => string("/data/id"),
        _ => string("/data/issueId").or_else(|| string("/data/issue/id")),
    };
    Ok(LinearWebhookEnvelope {
        delivery_id: delivery_header
            .map(str::to_owned)
            .unwrap_or_else(|| derived_delivery_id(body)),
        event_type,
        action,
        organization_id,
        webhook_id,
        webhook_timestamp_millis,
        issue_id,
        team_id: string("/data/teamId").or_else(|| string("/data/team/id")),
        actor_id: string("/actor/id"),
    })
}

/// Fallback de-duplication key for a delivery without the header. Identical
/// bodies carry the same Linear timestamp, so collapsing them is correct.
fn derived_delivery_id(body: &[u8]) -> String {
    use sha2::Digest;
    format!("sha256:{}", hex::encode(Sha256::digest(body)))
}

/// Event types worth processing. Anything else is stored and acknowledged so
/// Linear stops retrying, then completed without a canonical fetch.
pub fn relevant_event_types() -> BTreeSet<String> {
    ["Issue", "IssueLabel", "Comment"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"webhook-secret";

    fn signature(body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(SECRET).unwrap();
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    fn body(timestamp_millis: i64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "type": "Issue",
            "action": "update",
            "organizationId": "org",
            "webhookId": "hook",
            "webhookTimestamp": timestamp_millis,
            "actor": {"id": "actor"},
            "data": {"id": "issue-1", "teamId": "team", "description": "untrusted instruction"}
        }))
        .unwrap()
    }

    fn request<'a>(body: &'a [u8], signature: &'a str) -> WebhookRequest<'a> {
        WebhookRequest {
            method: "POST",
            path: "/webhooks/linear",
            content_type: Some("application/json; charset=utf-8"),
            signature: Some(signature),
            delivery_id: Some("delivery-1"),
            declared_length: Some(body.len()),
            body,
        }
    }

    fn accept(
        request: &WebhookRequest<'_>,
        now: i64,
    ) -> Result<AcceptedDelivery, WebhookRejection> {
        accept_delivery(
            request,
            "/webhooks/linear",
            WebhookLimits {
                max_body_bytes: 1024,
                replay_window_seconds: 60,
            },
            SECRET,
            WebhookAllowlist {
                organization_id: "org",
                webhook_id: "hook",
            },
            now,
        )
    }

    #[test]
    fn accepts_a_valid_signed_delivery_without_retaining_issue_text() {
        let body = body(1_000_000_000_000);
        let accepted = accept(&request(&body, &signature(&body)), 1_000_000_000).unwrap();
        assert_eq!(accepted.envelope.delivery_id, "delivery-1");
        assert_eq!(accepted.envelope.issue_id.as_deref(), Some("issue-1"));
        assert_eq!(accepted.envelope.team_id.as_deref(), Some("team"));
        assert_eq!(accepted.envelope.actor_id.as_deref(), Some("actor"));
        let rendered = serde_json::to_string(&accepted).unwrap();
        assert!(!rendered.contains("untrusted instruction"));
        assert!(!rendered.contains(&signature(&body)));
    }

    #[test]
    fn rejects_every_boundary_violation_before_parsing_state() {
        let body = body(1_000_000_000_000);
        let valid = signature(&body);
        let cases: [(WebhookRequest<'_>, WebhookRejection); 7] = [
            (
                WebhookRequest {
                    method: "GET",
                    ..request(&body, &valid)
                },
                WebhookRejection::MethodNotAllowed,
            ),
            (
                WebhookRequest {
                    path: "/webhooks/other",
                    ..request(&body, &valid)
                },
                WebhookRejection::PathNotFound,
            ),
            (
                WebhookRequest {
                    content_type: Some("text/plain"),
                    ..request(&body, &valid)
                },
                WebhookRejection::UnsupportedMediaType,
            ),
            (
                WebhookRequest {
                    declared_length: Some(4096),
                    ..request(&body, &valid)
                },
                WebhookRejection::BodyTooLarge,
            ),
            (
                WebhookRequest {
                    signature: None,
                    ..request(&body, &valid)
                },
                WebhookRejection::MissingSignature,
            ),
            (
                WebhookRequest {
                    signature: Some("00"),
                    ..request(&body, &valid)
                },
                WebhookRejection::InvalidSignature,
            ),
            (
                WebhookRequest {
                    signature: Some(&valid),
                    body: b"{",
                    declared_length: Some(1),
                    ..request(&body, &valid)
                },
                WebhookRejection::InvalidSignature,
            ),
        ];
        for (request, expected) in cases {
            assert_eq!(accept(&request, 1_000_000_000), Err(expected));
        }
    }

    #[test]
    fn malformed_json_and_stale_or_foreign_deliveries_are_refused() {
        let malformed = br#"{"type":"Issue"}"#.to_vec();
        assert_eq!(
            accept(&request(&malformed, &signature(&malformed)), 1_000_000_000),
            Err(WebhookRejection::MalformedPayload)
        );
        let stale = body(1_000_000_000_000);
        assert_eq!(
            accept(&request(&stale, &signature(&stale)), 1_000_000_120),
            Err(WebhookRejection::StaleTimestamp)
        );
        let future = body(1_000_000_120_000);
        assert_eq!(
            accept(&request(&future, &signature(&future)), 1_000_000_000),
            Err(WebhookRejection::StaleTimestamp)
        );
        let foreign = serde_json::to_vec(&serde_json::json!({
            "type": "Issue", "action": "update", "organizationId": "other",
            "webhookId": "hook", "webhookTimestamp": 1_000_000_000_000i64, "data": {"id": "issue-1"}
        }))
        .unwrap();
        assert_eq!(
            accept(&request(&foreign, &signature(&foreign)), 1_000_000_000),
            Err(WebhookRejection::UnknownOrganization)
        );
    }

    #[test]
    fn a_missing_delivery_header_still_produces_a_stable_key() {
        let body = body(1_000_000_000_000);
        let signature = signature(&body);
        let mut request = request(&body, &signature);
        request.delivery_id = None;
        let first = accept(&request, 1_000_000_000).unwrap();
        let second = accept(&request, 1_000_000_000).unwrap();
        assert_eq!(first.envelope.delivery_id, second.envelope.delivery_id);
        assert!(first.envelope.delivery_id.starts_with("sha256:"));
    }

    #[test]
    fn comment_events_resolve_the_issue_they_belong_to() {
        let body = serde_json::to_vec(&serde_json::json!({
            "type": "Comment", "action": "create", "organizationId": "org", "webhookId": "hook",
            "webhookTimestamp": 1_000_000_000_000i64, "data": {"id": "comment-1", "issueId": "issue-9"}
        }))
        .unwrap();
        let accepted = accept(&request(&body, &signature(&body)), 1_000_000_000).unwrap();
        assert_eq!(accepted.envelope.issue_id.as_deref(), Some("issue-9"));
        assert!(relevant_event_types().contains(&accepted.envelope.event_type));
    }
}
