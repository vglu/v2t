//! Per-host request pacing + escalating cooldown after rate-limit responses.
//!
//! Every yt-dlp invocation that targets a URL should call `HostGate::acquire`
//! before spawning the process and `HostGate::report` after it finishes.
//! `acquire` sleeps the minimum time needed to honour both the inter-request
//! interval and any active cooldown, then records the spawn instant.
//! `report` updates the cooldown state based on the outcome.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use rand::Rng;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::pipeline::JOB_CANCELLED_MSG;
use crate::settings::AppSettings;

/// Pacing knobs derived from `AppSettings`.
#[derive(Debug, Clone)]
pub struct PacingConfig {
    /// Minimum gap between two yt-dlp spawns for the same host.
    pub min_interval: Duration,
    /// Maximum random jitter added on top of `min_interval`.
    pub jitter: Duration,
    /// Cooldown applied to the host after the first 429 response.
    pub cooldown_on_429: Duration,
    /// Base for the exponential backoff between retries.
    pub backoff_base: Duration,
    /// Upper cap for both backoff and cooldown.
    pub backoff_max: Duration,
    /// Maximum per-item download attempts (including the first try).
    pub max_attempts: u8,
}

impl PacingConfig {
    pub fn from_settings(s: &AppSettings) -> Self {
        Self {
            min_interval: Duration::from_millis(s.download_min_interval_ms as u64),
            jitter: Duration::from_millis(s.download_jitter_ms as u64),
            cooldown_on_429: Duration::from_millis(s.download_cooldown_ms as u64),
            backoff_base: Duration::from_millis(s.download_backoff_base_ms as u64),
            backoff_max: Duration::from_millis(s.download_backoff_max_ms as u64),
            max_attempts: s.download_max_attempts,
        }
    }
}

/// Outcome of a single yt-dlp run, passed to `HostGate::report`.
pub enum PacingOutcome {
    Ok,
    /// HTTP 429 — explicit rate limit.
    RateLimited,
    /// HTTP 503 / 403 or similar transient server error.
    ServerBusy,
}

#[derive(Debug, Default)]
struct HostState {
    /// When the last yt-dlp spawn was initiated for this host.
    last_spawn: Option<Instant>,
    /// If `Some`, the host is in cooldown; `acquire` will wait until this instant.
    cooldown_until: Option<Instant>,
    /// How many times RateLimited was reported since last success (doubles cooldown).
    cooldown_escalation: u32,
    /// Consecutive ServerBusy responses; after 3 a half-cooldown is applied.
    consecutive_server_errors: u32,
}

/// Thread-safe per-host request gate.
///
/// Register as Tauri state via `.manage(HostGate::new())`. Pass a reference to
/// every function that spawns yt-dlp for a URL source.
pub struct HostGate {
    state: Mutex<HashMap<String, HostState>>,
}

impl Default for HostGate {
    fn default() -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
        }
    }
}

impl HostGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wait until the gate opens for `host`, then record the spawn instant.
    ///
    /// Respects `cancel`: returns `Err(JOB_CANCELLED_MSG)` if the token fires.
    pub async fn acquire(
        &self,
        host: &str,
        cfg: &PacingConfig,
        cancel: &CancellationToken,
    ) -> Result<(), String> {
        let sleep_dur = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let entry = state.entry(host.to_string()).or_default();
            let now = Instant::now();

            // Jitter is randomised per-call so consecutive items don't fire in lockstep.
            let jitter_ms: u64 = if cfg.jitter.is_zero() {
                0
            } else {
                rand::thread_rng().gen_range(0..=cfg.jitter.as_millis() as u64)
            };
            let jitter = Duration::from_millis(jitter_ms);

            // How long until the active cooldown expires?
            let cooldown_remaining = entry
                .cooldown_until
                .filter(|&t| t > now)
                .map(|t| t - now)
                .unwrap_or(Duration::ZERO);

            // How long until the minimum interval has passed since last spawn?
            let interval_remaining = entry
                .last_spawn
                .map(|last| {
                    let elapsed = now.saturating_duration_since(last);
                    let required = cfg.min_interval + jitter;
                    required.saturating_sub(elapsed)
                })
                .unwrap_or(Duration::ZERO);

            cooldown_remaining.max(interval_remaining)
        };

        if !sleep_dur.is_zero() {
            tokio::select! {
                _ = tokio::time::sleep(sleep_dur) => {}
                _ = cancel.cancelled() => return Err(JOB_CANCELLED_MSG.to_string()),
            }
        }

        // Record spawn instant after the wait so the *next* acquire's interval is
        // measured from when yt-dlp actually starts, not when `acquire` was called.
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let entry = state.entry(host.to_string()).or_default();
        entry.last_spawn = Some(Instant::now());

        Ok(())
    }

    /// Update host state based on the result of the yt-dlp run.
    ///
    /// - `Ok` — resets cooldown counter; future requests get the normal interval.
    /// - `RateLimited` — sets an escalating cooldown (doubles on each subsequent 429).
    /// - `ServerBusy` — increments a consecutive-error counter; after 3 in a row
    ///   applies a half-sized cooldown.
    pub fn report(&self, host: &str, outcome: PacingOutcome, cfg: &PacingConfig) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let entry = state.entry(host.to_string()).or_default();
        let now = Instant::now();

        match outcome {
            PacingOutcome::Ok => {
                entry.cooldown_until = None;
                entry.cooldown_escalation = 0;
                entry.consecutive_server_errors = 0;
            }
            PacingOutcome::RateLimited => {
                // Escalating cooldown: cooldown_on_429 * 2^escalation, capped at backoff_max.
                let shift = entry.cooldown_escalation.min(30);
                let multiplier = 1u32 << shift;
                let raw_ms =
                    cfg.cooldown_on_429.as_millis() as u64 * multiplier as u64;
                let capped_ms = raw_ms.min(cfg.backoff_max.as_millis() as u64);
                entry.cooldown_until = Some(now + Duration::from_millis(capped_ms));
                entry.cooldown_escalation = entry.cooldown_escalation.saturating_add(1);
                entry.consecutive_server_errors = 0;
            }
            PacingOutcome::ServerBusy => {
                entry.consecutive_server_errors =
                    entry.consecutive_server_errors.saturating_add(1);
                if entry.consecutive_server_errors >= 3 {
                    let half_cooldown_ms =
                        (cfg.cooldown_on_429.as_millis() as u64 / 2)
                            .min(cfg.backoff_max.as_millis() as u64);
                    // Only set cooldown if it's not already longer.
                    let proposed = now + Duration::from_millis(half_cooldown_ms);
                    match entry.cooldown_until {
                        None => entry.cooldown_until = Some(proposed),
                        Some(existing) if proposed > existing => {
                            entry.cooldown_until = Some(proposed);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Extract the host portion of a URL without external crates.
///
/// `"https://www.tiktok.com/@user/video/1"` → `"www.tiktok.com"`
pub fn extract_host(url: &str) -> String {
    let trimmed = url.trim();
    let rest = if let Some(r) = trimmed.strip_prefix("https://") {
        r
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        r
    } else {
        return trimmed.to_string();
    };
    match rest.find('/') {
        Some(pos) => rest[..pos].to_string(),
        None => rest.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn test_cfg() -> PacingConfig {
        PacingConfig {
            min_interval: Duration::from_millis(100),
            jitter: Duration::ZERO,
            cooldown_on_429: Duration::from_millis(200),
            backoff_base: Duration::from_millis(50),
            backoff_max: Duration::from_millis(1_000),
            max_attempts: 4,
        }
    }

    #[test]
    fn extract_host_basic() {
        assert_eq!(
            extract_host("https://www.tiktok.com/@u/video/1"),
            "www.tiktok.com"
        );
        assert_eq!(
            extract_host("http://example.com/path?q=1"),
            "example.com"
        );
        assert_eq!(extract_host("not-a-url"), "not-a-url");
        assert_eq!(
            extract_host("https://youtu.be/abc"),
            "youtu.be"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn first_acquire_no_wait() {
        let gate = HostGate::new();
        let cfg = test_cfg();
        let cancel = CancellationToken::new();

        gate.acquire("tiktok.com", &cfg, &cancel).await.unwrap();

        let state = gate.state.lock().unwrap();
        assert!(state.get("tiktok.com").unwrap().last_spawn.is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn min_interval_enforced() {
        let gate = Arc::new(HostGate::new());
        let cfg = test_cfg(); // min_interval = 100ms
        let cancel = CancellationToken::new();

        gate.acquire("tiktok.com", &cfg, &cancel).await.unwrap();

        // Second acquire before 100ms should wait.
        let gate2 = Arc::clone(&gate);
        let cfg2 = cfg.clone();
        let cancel2 = cancel.clone();
        let handle = tokio::spawn(async move {
            gate2.acquire("tiktok.com", &cfg2, &cancel2).await.unwrap();
        });

        // Advance virtual time past min_interval.
        tokio::time::advance(Duration::from_millis(101)).await;
        handle.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn different_hosts_are_independent() {
        let gate = HostGate::new();
        let cfg = test_cfg();
        let cancel = CancellationToken::new();

        gate.acquire("tiktok.com", &cfg, &cancel).await.unwrap();
        // youtube.com has never been seen — should not wait.
        gate.acquire("youtube.com", &cfg, &cancel).await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limited_sets_cooldown() {
        let gate = HostGate::new();
        let cfg = test_cfg(); // cooldown_on_429 = 200ms
        let cancel = CancellationToken::new();

        gate.acquire("tiktok.com", &cfg, &cancel).await.unwrap();
        gate.report("tiktok.com", PacingOutcome::RateLimited, &cfg);

        let state = gate.state.lock().unwrap();
        let entry = state.get("tiktok.com").unwrap();
        assert!(entry.cooldown_until.is_some());
        // cooldown_escalation should advance to 1 (next call will double).
        assert_eq!(entry.cooldown_escalation, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn cooldown_escalates_on_repeated_rate_limit() {
        let gate = HostGate::new();
        let cfg = test_cfg(); // cooldown_on_429 = 200ms
        let cancel = CancellationToken::new();

        // First 429
        gate.acquire("tiktok.com", &cfg, &cancel).await.unwrap();
        gate.report("tiktok.com", PacingOutcome::RateLimited, &cfg);
        let first_until = gate
            .state
            .lock()
            .unwrap()
            .get("tiktok.com")
            .unwrap()
            .cooldown_until
            .unwrap();

        // Second 429
        gate.report("tiktok.com", PacingOutcome::RateLimited, &cfg);
        let second_until = gate
            .state
            .lock()
            .unwrap()
            .get("tiktok.com")
            .unwrap()
            .cooldown_until
            .unwrap();

        // Second cooldown should be later (longer) than the first.
        assert!(second_until > first_until);
    }

    #[tokio::test(start_paused = true)]
    async fn success_resets_cooldown() {
        let gate = HostGate::new();
        let cfg = test_cfg();
        let cancel = CancellationToken::new();

        gate.acquire("tiktok.com", &cfg, &cancel).await.unwrap();
        gate.report("tiktok.com", PacingOutcome::RateLimited, &cfg);
        gate.report("tiktok.com", PacingOutcome::Ok, &cfg);

        let state = gate.state.lock().unwrap();
        let entry = state.get("tiktok.com").unwrap();
        assert!(entry.cooldown_until.is_none());
        assert_eq!(entry.cooldown_escalation, 0);
        assert_eq!(entry.consecutive_server_errors, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn server_busy_triggers_cooldown_after_3_errors() {
        let gate = HostGate::new();
        let cfg = test_cfg(); // cooldown_on_429 = 200ms, half = 100ms
        let cancel = CancellationToken::new();

        gate.acquire("tiktok.com", &cfg, &cancel).await.unwrap();
        gate.report("tiktok.com", PacingOutcome::ServerBusy, &cfg);
        gate.report("tiktok.com", PacingOutcome::ServerBusy, &cfg);

        {
            let state = gate.state.lock().unwrap();
            let entry = state.get("tiktok.com").unwrap();
            // Only 2 errors — no cooldown yet.
            assert!(entry.cooldown_until.is_none());
        }

        gate.report("tiktok.com", PacingOutcome::ServerBusy, &cfg);

        {
            let state = gate.state.lock().unwrap();
            let entry = state.get("tiktok.com").unwrap();
            // 3rd error — half-cooldown applied.
            assert!(entry.cooldown_until.is_some());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cooldown_respected_in_acquire() {
        let gate = Arc::new(HostGate::new());
        let cfg = test_cfg(); // cooldown_on_429 = 200ms
        let cancel = CancellationToken::new();

        gate.acquire("tiktok.com", &cfg, &cancel).await.unwrap();
        gate.report("tiktok.com", PacingOutcome::RateLimited, &cfg);

        let gate2 = Arc::clone(&gate);
        let cfg2 = cfg.clone();
        let cancel2 = cancel.clone();
        let handle = tokio::spawn(async move {
            gate2.acquire("tiktok.com", &cfg2, &cancel2).await.unwrap();
        });

        // Advance past the 200ms cooldown.
        tokio::time::advance(Duration::from_millis(201)).await;
        handle.await.unwrap();
    }
}
