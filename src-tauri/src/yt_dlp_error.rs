//! Error classification for yt-dlp failures.
//!
//! Parses stderr output and assigns a `DownloadFailureKind` so callers can decide
//! whether to retry, surface a hint, or fail immediately.

use serde::Serialize;
use std::time::Duration;

use crate::net_pacing::PacingConfig;

/// Chromium cookie DB lock / app-bound encryption on Windows (yt-dlp #7271 / #10927).
/// Moved here from `pipeline.rs` so `classify` can detect it without a circular import.
pub fn yt_dlp_cookie_db_failure(stderr: &[u8]) -> bool {
    let s = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    s.contains("could not copy") && s.contains("cookie")
        || s.contains("cookie database") && (s.contains("permission denied") || s.contains("failed to decrypt"))
        || s.contains("failed to decrypt with dpapi")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadFailureKind {
    /// HTTP 429 — explicit rate limit.
    RateLimited,
    /// Site rejected this client IP for the post (e.g. TikTok). Not worth retrying.
    IpBlocked,
    /// HTTP 502 / 503 / 504 — server overloaded.
    ServiceUnavailable,
    /// HTTP 403 — session/shield rejection; often clears after a pause.
    Forbidden,
    /// "Unsupported URL" — e.g. TikTok photo posts; no point retrying.
    Unsupported,
    /// 404, private video, removed — permanently gone.
    Unavailable,
    /// "Unable to extract universal data for rehydration" — yt-dlp is too old.
    ExtractorStale,
    /// Browser cookie DB locked or encrypted; handled separately by existing retry.
    CookieStore,
    /// Heartbeat watchdog killed yt-dlp (no output for N seconds).
    Stalled,
    /// Any other non-zero exit code.
    Other,
}

/// Classify a yt-dlp failure from its stderr bytes.
pub fn classify(stderr: &[u8]) -> DownloadFailureKind {
    if yt_dlp_cookie_db_failure(stderr) {
        return DownloadFailureKind::CookieStore;
    }

    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();

    // TikTok (and similar) IP bans: further attempts on the same post are pointless.
    if text.contains("ip address is blocked")
        || text.contains("your ip address is blocked")
        || text.contains("blocked from accessing this post")
    {
        return DownloadFailureKind::IpBlocked;
    }
    if text.contains("http error 429") {
        return DownloadFailureKind::RateLimited;
    }
    if text.contains("http error 503")
        || text.contains("http error 502")
        || text.contains("http error 504")
    {
        return DownloadFailureKind::ServiceUnavailable;
    }
    if text.contains("http error 403") {
        return DownloadFailureKind::Forbidden;
    }
    if text.contains("unable to extract universal data for rehydration") {
        return DownloadFailureKind::ExtractorStale;
    }
    if text.contains("unsupported url") {
        return DownloadFailureKind::Unsupported;
    }
    if text.contains("http error 404")
        || text.contains("video not available")
        || text.contains("this video is private")
        || text.contains("video is unavailable")
        || text.contains("this video has been removed")
    {
        return DownloadFailureKind::Unavailable;
    }

    DownloadFailureKind::Other
}

/// Classify a Rust-level error string returned by the yt-dlp runner (not stderr).
/// Used to detect stalled downloads that `classify(stderr)` cannot see.
pub fn classify_runner_error(err: &str) -> DownloadFailureKind {
    if err.to_ascii_lowercase().contains("stalled") {
        DownloadFailureKind::Stalled
    } else {
        DownloadFailureKind::Other
    }
}

impl DownloadFailureKind {
    /// Whether it makes sense to pause and retry after this error.
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            DownloadFailureKind::RateLimited
                | DownloadFailureKind::ServiceUnavailable
                | DownloadFailureKind::Forbidden
                | DownloadFailureKind::Stalled
        )
    }

    /// Permanent for this URL right now — do not fall through to another yt-dlp stage.
    pub fn is_fatal_for_url(self) -> bool {
        matches!(
            self,
            DownloadFailureKind::IpBlocked
                | DownloadFailureKind::Unsupported
                | DownloadFailureKind::Unavailable
        )
    }

    /// Exponential backoff duration before retry attempt `attempt` (1-based).
    /// Formula: `backoff_base * 2^(attempt-1)`, capped at `backoff_max`.
    pub fn backoff(self, attempt: u32, cfg: &PacingConfig) -> Duration {
        let base_ms = cfg.backoff_base.as_millis() as u64;
        let shift = attempt.saturating_sub(1);
        let multiplied = base_ms.saturating_mul(1u64 << shift.min(62));
        let capped = multiplied.min(cfg.backoff_max.as_millis() as u64);
        Duration::from_millis(capped)
    }

    /// Short camelCase tag for embedding in error messages.
    pub fn label(self) -> &'static str {
        match self {
            DownloadFailureKind::RateLimited => "rateLimited",
            DownloadFailureKind::IpBlocked => "ipBlocked",
            DownloadFailureKind::ServiceUnavailable => "serviceUnavailable",
            DownloadFailureKind::Forbidden => "forbidden",
            DownloadFailureKind::Unsupported => "unsupported",
            DownloadFailureKind::Unavailable => "unavailable",
            DownloadFailureKind::ExtractorStale => "extractorStale",
            DownloadFailureKind::CookieStore => "cookieStore",
            DownloadFailureKind::Stalled => "stalled",
            DownloadFailureKind::Other => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PacingConfig {
        PacingConfig {
            min_interval: Duration::from_millis(100),
            jitter: Duration::ZERO,
            cooldown_on_429: Duration::from_millis(60_000),
            backoff_base: Duration::from_millis(5_000),
            backoff_max: Duration::from_millis(120_000),
            max_attempts: 4,
        }
    }

    #[test]
    fn classify_rate_limited() {
        let kind = classify(b"ERROR: HTTP Error 429: Too Many Requests");
        assert_eq!(kind, DownloadFailureKind::RateLimited);
        assert!(kind.is_retryable());
    }

    #[test]
    fn classify_tiktok_ip_blocked_is_fatal_not_retryable() {
        let kind = classify(
            b"ERROR: [TikTok] 7624508884811582728: Your IP address is blocked from accessing this post",
        );
        assert_eq!(kind, DownloadFailureKind::IpBlocked);
        assert!(!kind.is_retryable());
        assert!(kind.is_fatal_for_url());
    }

    #[test]
    fn classify_service_unavailable_503() {
        let kind = classify(b"ERROR: HTTP Error 503: Service Unavailable");
        assert_eq!(kind, DownloadFailureKind::ServiceUnavailable);
        assert!(kind.is_retryable());
    }

    #[test]
    fn classify_forbidden_403() {
        let kind = classify(b"ERROR: HTTP Error 403: Forbidden");
        assert_eq!(kind, DownloadFailureKind::Forbidden);
        assert!(kind.is_retryable());
    }

    #[test]
    fn classify_unsupported_url() {
        let kind = classify(b"ERROR: Unsupported URL: https://www.tiktok.com/@user/photo/1234");
        assert_eq!(kind, DownloadFailureKind::Unsupported);
        assert!(!kind.is_retryable());
    }

    #[test]
    fn classify_extractor_stale() {
        let kind = classify(
            b"ERROR: Unable to extract universal data for rehydration",
        );
        assert_eq!(kind, DownloadFailureKind::ExtractorStale);
        assert!(!kind.is_retryable());
    }

    #[test]
    fn classify_stalled_via_runner_error() {
        let kind = classify_runner_error("yt-dlp stalled (no output for 120s) — network issue");
        assert_eq!(kind, DownloadFailureKind::Stalled);
        assert!(kind.is_retryable());
    }

    #[test]
    fn classify_cookie_db_failure() {
        let kind = classify(b"ERROR: Could not copy Chrome cookie database");
        assert_eq!(kind, DownloadFailureKind::CookieStore);
        assert!(!kind.is_retryable());
    }

    #[test]
    fn classify_other_for_unknown_error() {
        let kind = classify(b"ERROR: Some unknown yt-dlp failure");
        assert_eq!(kind, DownloadFailureKind::Other);
        assert!(!kind.is_retryable());
    }

    #[test]
    fn backoff_exponential_growth() {
        let c = cfg();
        // attempt 1: 5000ms
        assert_eq!(
            DownloadFailureKind::RateLimited.backoff(1, &c),
            Duration::from_millis(5_000)
        );
        // attempt 2: 10000ms
        assert_eq!(
            DownloadFailureKind::RateLimited.backoff(2, &c),
            Duration::from_millis(10_000)
        );
        // attempt 3: 20000ms
        assert_eq!(
            DownloadFailureKind::RateLimited.backoff(3, &c),
            Duration::from_millis(20_000)
        );
    }

    #[test]
    fn backoff_caps_at_max() {
        let c = cfg(); // backoff_max = 120_000ms
        // attempt 5: 5000 * 16 = 80000ms, still under cap
        assert_eq!(
            DownloadFailureKind::RateLimited.backoff(5, &c),
            Duration::from_millis(80_000)
        );
        // attempt 6: 5000 * 32 = 160000ms → capped at 120000ms
        assert_eq!(
            DownloadFailureKind::RateLimited.backoff(6, &c),
            Duration::from_millis(120_000)
        );
    }

    #[test]
    fn labels_are_camel_case() {
        assert_eq!(DownloadFailureKind::RateLimited.label(), "rateLimited");
        assert_eq!(DownloadFailureKind::IpBlocked.label(), "ipBlocked");
        assert_eq!(DownloadFailureKind::ServiceUnavailable.label(), "serviceUnavailable");
        assert_eq!(DownloadFailureKind::ExtractorStale.label(), "extractorStale");
        assert_eq!(DownloadFailureKind::CookieStore.label(), "cookieStore");
    }
}
