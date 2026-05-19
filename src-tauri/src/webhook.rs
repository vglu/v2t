//! Outgoing webhook delivery for the REST API (M2 wave).
//!
//! Single-shot POST with HMAC-SHA256 signature in `X-V2T-Signature` and an
//! idempotency token in `X-V2T-Delivery-Id`. Retries with exponential backoff
//! on transport errors and 5xx; best-effort overall — failures are reported
//! back to the caller but never block the originating job.

use std::time::Duration;

use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

const MAX_ATTEMPTS: u32 = 3;
const BASE_BACKOFF: Duration = Duration::from_millis(500);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Delivery target metadata captured at job-submission time. Held in
/// `ApiJob.callback` so the dispatcher can read it without locking the registry
/// for the duration of the round-trip.
#[derive(Debug, Clone)]
pub struct WebhookTarget {
    pub url: String,
    pub secret: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookPayload<'a, T: Serialize> {
    pub event: &'a str,
    pub job_id: &'a str,
    pub data: T,
}

fn sign_body(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key size");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// Deliver a single webhook event. Returns `Ok(())` on 2xx, `Err(msg)` on the
/// final failure after all retries.
pub async fn deliver<T: Serialize>(
    target: &WebhookTarget,
    event: &str,
    job_id: &str,
    data: T,
) -> Result<(), String> {
    let body_struct = WebhookPayload { event, job_id, data };
    let body_bytes =
        serde_json::to_vec(&body_struct).map_err(|e| format!("webhook serialize: {e}"))?;
    let delivery_id = Uuid::new_v4().to_string();

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("webhook client: {e}"))?;

    let mut last_err = String::new();

    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            // Exponential backoff with a small base — total wait under 2s for 3 tries.
            tokio::time::sleep(BASE_BACKOFF * (1 << (attempt - 1))).await;
        }

        let mut req = client
            .post(&target.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("X-V2T-Delivery-Id", &delivery_id)
            .header("X-V2T-Event", event);

        if let Some(ref secret) = target.secret {
            if !secret.is_empty() {
                req = req.header("X-V2T-Signature", sign_body(secret, &body_bytes));
            }
        }

        match req.body(body_bytes.clone()).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return Ok(());
                }
                last_err = format!("webhook HTTP {status}");
                // Don't retry 4xx — they won't get better.
                if status.is_client_error() {
                    return Err(last_err);
                }
            }
            Err(e) => {
                last_err = format!("webhook transport: {e}");
            }
        }
    }

    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_signature_is_stable_hex() {
        let sig1 = sign_body("secret", b"{\"a\":1}");
        let sig2 = sign_body("secret", b"{\"a\":1}");
        assert_eq!(sig1, sig2);
        assert_eq!(sig1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn hmac_changes_with_different_secret() {
        let a = sign_body("k1", b"hello");
        let b = sign_body("k2", b"hello");
        assert_ne!(a, b);
    }

    #[test]
    fn hmac_changes_with_different_body() {
        let a = sign_body("k", b"hello");
        let b = sign_body("k", b"world");
        assert_ne!(a, b);
    }
}
