use std::time::Duration;
use rand::Rng;

const INITIAL_DELAY_MS: u64 = 200;

pub(crate) fn backoff(attempt: u64) -> Duration {
    let base = INITIAL_DELAY_MS << attempt.saturating_sub(1).min(10); // Cap at 2^10 to prevent overflow
    let jitter_factor = rand::rng().random_range(90..=110); // 0.9 to 1.1 as percentage
    Duration::from_millis(base * jitter_factor / 100)
}
