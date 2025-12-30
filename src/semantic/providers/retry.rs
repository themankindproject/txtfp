//! Retry / backoff for the cloud-provider HTTP paths.
//!
//! Hand-rolled exponential backoff with jitter, plus explicit
//! `Retry-After` honoring on HTTP 429. Implementing this inline (vs
//! pulling a third-party `backoff`/`backon` dependency) keeps the
//! transitive surface minimal — both `backoff 0.4` and `instant`
//! (its transitive) are flagged unmaintained as of 2025/2026, and
//! switching to `backon` would force a tokio-native API even on the
//! blocking path.
//!
//! # Retry policy
//!
//! - **Transient (retried)**: HTTP 408, 425, 429, 500, 502, 503, 504,
//!   network errors (timeout, connection refused, DNS failure).
//! - **Permanent (not retried)**: HTTP 400, 401, 403, 404, 422, and any
//!   2xx response with a malformed body.
//! - **Initial backoff**: 500 ms; multiplier 2.0; full-jitter
//!   randomization; max single sleep 30 s; max wall-clock budget 90 s.
//! - **`Retry-After`**: when present on a 429 response, the worker
//!   sleeps for the indicated duration (capped at 60 s) before the
//!   next attempt, overriding the exponential schedule for that step.

use core::time::Duration;
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder, Response};

use crate::error::Error;

const INITIAL_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const MAX_ELAPSED: Duration = Duration::from_secs(90);
const MULTIPLIER: f32 = 2.0;
/// Jitter band: each sleep is uniformly drawn from
/// `[backoff * (1 - JITTER), backoff * (1 + JITTER)]`.
const JITTER: f32 = 0.3;
/// Hard cap on a single `Retry-After` sleep.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// Send the request via `client`, retrying transient failures.
///
/// `build_request` is invoked for every attempt because
/// `reqwest::blocking::RequestBuilder` is consumed by `send()`.
pub(super) fn send_with_retry<F>(
    _client: &Client,
    mut build_request: F,
    provider: &'static str,
) -> Result<Response, Error>
where
    F: FnMut() -> RequestBuilder,
{
    let started = Instant::now();
    let mut backoff = INITIAL_BACKOFF;

    loop {
        match attempt(&mut build_request, provider) {
            AttemptOutcome::Success(resp) => return Ok(resp),
            AttemptOutcome::Permanent(err) => return Err(err),
            AttemptOutcome::Transient { err, retry_after } => {
                let elapsed = started.elapsed();
                if elapsed >= MAX_ELAPSED {
                    return Err(err);
                }
                let sleep = retry_after
                    .map(|d| d.min(MAX_RETRY_AFTER))
                    .unwrap_or_else(|| jitter(backoff));
                let remaining = MAX_ELAPSED.saturating_sub(elapsed);
                thread::sleep(sleep.min(remaining));
                backoff = (backoff.mul_f32(MULTIPLIER)).min(MAX_BACKOFF);
            }
        }
    }
}

enum AttemptOutcome {
    Success(Response),
    Permanent(Error),
    Transient {
        err: Error,
        retry_after: Option<Duration>,
    },
}

fn attempt<F>(build_request: &mut F, provider: &'static str) -> AttemptOutcome
where
    F: FnMut() -> RequestBuilder,
{
    let resp = match build_request().send() {
        Ok(r) => r,
        Err(e) => {
            let err = Error::Http(format!("{provider} send: {e}"));
            // `is_builder` is the only deterministically-permanent variant.
            return if e.is_builder() {
                AttemptOutcome::Permanent(err)
            } else {
                AttemptOutcome::Transient {
                    err,
                    retry_after: None,
                }
            };
        }
    };

    let status = resp.status();
    match classify(status) {
        Class::Success => AttemptOutcome::Success(resp),
        Class::PermanentClient => {
            AttemptOutcome::Permanent(Error::Http(format!("{provider} returned {status}")))
        }
        Class::Transient => {
            let retry_after = parse_retry_after(&resp);
            let err = if let Some(d) = retry_after {
                Error::Http(format!("{provider} returned {status} (Retry-After {d:?})"))
            } else {
                Error::Http(format!("{provider} returned {status}"))
            };
            AttemptOutcome::Transient { err, retry_after }
        }
    }
}

#[derive(Copy, Clone, Debug)]
enum Class {
    Success,
    PermanentClient,
    Transient,
}

fn classify(status: StatusCode) -> Class {
    if status.is_success() {
        Class::Success
    } else if matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504) {
        Class::Transient
    } else {
        Class::PermanentClient
    }
}

/// Apply `±JITTER` to a backoff duration.
///
/// We don't pull in `rand` for this — the only requirement is that
/// concurrent retriers don't synchronize. A cheap PRNG seeded from the
/// monotonic clock is more than sufficient and avoids the dep.
fn jitter(d: Duration) -> Duration {
    // 64-bit splitmix from the current nanosecond clock.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|x| x.subsec_nanos() as u64)
        .unwrap_or(0)
        ^ Instant::now().elapsed().subsec_nanos() as u64;
    let mut z = nanos.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // Map to [1 - JITTER, 1 + JITTER]. 53 bits of mantissa precision
    // is plenty.
    let unit = (z >> 11) as f32 / (1u64 << 53) as f32;
    let factor = (1.0 - JITTER) + 2.0 * JITTER * unit;
    d.mul_f32(factor)
}

/// Parse the `Retry-After` header.
///
/// Per RFC 7231, the value is either an HTTP-date (which we don't try
/// to decode here) or a delta-seconds integer. We accept the integer
/// form and ignore date forms.
fn parse_retry_after(resp: &Response) -> Option<Duration> {
    let hv = resp.headers().get(reqwest::header::RETRY_AFTER)?;
    let s = hv.to_str().ok()?;
    let n: u64 = s.trim().parse().ok()?;
    Some(Duration::from_secs(n))
}

use alloc::format;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifier_lumps_5xx_as_transient() {
        for s in [500u16, 502, 503, 504] {
            let cls = classify(StatusCode::from_u16(s).unwrap());
            assert!(matches!(cls, Class::Transient), "{s}");
        }
    }

    #[test]
    fn classifier_lumps_429_408_425_as_transient() {
        for s in [408u16, 425, 429] {
            let cls = classify(StatusCode::from_u16(s).unwrap());
            assert!(matches!(cls, Class::Transient), "{s}");
        }
    }

    #[test]
    fn classifier_lumps_4xx_as_permanent() {
        for s in [400u16, 401, 403, 404, 422] {
            let cls = classify(StatusCode::from_u16(s).unwrap());
            assert!(matches!(cls, Class::PermanentClient), "{s}");
        }
    }

    #[test]
    fn classifier_lumps_2xx_as_success() {
        for s in [200u16, 201, 204] {
            let cls = classify(StatusCode::from_u16(s).unwrap());
            assert!(matches!(cls, Class::Success), "{s}");
        }
    }

    #[test]
    fn jitter_stays_in_band() {
        let base = Duration::from_millis(1000);
        for _ in 0..32 {
            let j = jitter(base);
            assert!(j >= Duration::from_millis(700));
            assert!(j <= Duration::from_millis(1300));
        }
    }
}
