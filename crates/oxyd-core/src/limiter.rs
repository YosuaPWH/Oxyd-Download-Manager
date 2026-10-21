//! Token bucket speed limiter whose rate can change while downloads run.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

#[derive(Debug)]
pub struct Limiter {
    /// Bytes per second; 0 means unlimited.
    rate: AtomicU64,
    state: Mutex<Bucket>,
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Limiter {
    pub fn new(rate: Option<u64>) -> Self {
        Limiter {
            rate: AtomicU64::new(rate.unwrap_or(0)),
            state: Mutex::new(Bucket {
                tokens: 0.0,
                last: Instant::now(),
            }),
        }
    }

    pub fn rate(&self) -> Option<u64> {
        Some(self.rate.load(Ordering::Relaxed)).filter(|&r| r > 0)
    }

    pub fn set_rate(&self, rate: Option<u64>) {
        self.rate.store(rate.unwrap_or(0), Ordering::Relaxed);
    }

    /// Wait until `n` bytes may be consumed. Tokens may go into debt so large reads are
    /// delayed proportionally instead of being starved.
    pub async fn acquire(&self, n: u64) {
        let rate = self.rate.load(Ordering::Relaxed);
        if rate == 0 || n == 0 {
            return;
        }
        let wait = {
            let mut b = self.state.lock().await;
            let now = Instant::now();
            let elapsed = now.duration_since(b.last).as_secs_f64();
            b.last = now;
            // Allow at most a quarter second of burst.
            let burst = rate as f64 / 4.0;
            b.tokens = (b.tokens + elapsed * rate as f64).min(burst);
            b.tokens -= n as f64;
            if b.tokens >= 0.0 {
                return;
            }
            Duration::from_secs_f64(-b.tokens / rate as f64)
        };
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unlimited_is_instant() {
        let l = Limiter::new(None);
        let t = Instant::now();
        for _ in 0..1000 {
            l.acquire(1 << 20).await;
        }
        assert!(t.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn limits_throughput() {
        let l = Limiter::new(Some(400_000));
        let t = Instant::now();
        for _ in 0..20 {
            l.acquire(20_000).await; // 400 KB total
        }
        let secs = t.elapsed().as_secs_f64();
        assert!((0.7..1.6).contains(&secs), "took {secs}s");
    }

    #[tokio::test]
    async fn rate_can_be_cleared() {
        let l = Limiter::new(Some(1));
        l.set_rate(None);
        assert_eq!(l.rate(), None);
        let t = Instant::now();
        l.acquire(1_000_000).await;
        assert!(t.elapsed() < Duration::from_millis(50));
    }
}
