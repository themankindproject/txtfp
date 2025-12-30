//! Shared retry / backoff machinery for the cloud-provider HTTP paths.
//!
//! Wraps a single blocking POST in [`backoff::retry`] with exponential
//! backoff + jitter, and explicitly honors the `Retry-After` header on
//! HTTP 429 responses.
//!
//! # Retry policy
//!
//! - **Transient (retried)**: HTTP 408, 425, 429, 500, 502, 503, 504,
//!   network errors (timeout, connection refused, DNS failure).
//! - **Permanent (not retried)**: HTTP 400, 401, 403, 404, 422, and any
//!   2xx response with malformed body.
//! - **Initial backoff**: 500 ms, multiplier 2.0, max interval 30 s,
//!   max elapsed 90 s.
//! - **`Retry-After`**: when present on a 429 response, the worker
//!   sleeps for the indicated duration before the next attempt
//!   (overriding the exponential schedule for that one step).

use core::time::Duration;

use backoff::{ExponentialBackoff, ExponentialBackoffBuilder, retry as backoff_retry};
use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder, Response};

use crate::error::Error;

/// Built-in default backoff schedule used by all cloud providers.
///
/// Tuned for typical embedding APIs (per-call latency 200 ms – 5 s,
/// 429s rare but should not collapse into a hot retry loop).
fn default_schedule() -> ExponentialBackoff {
    ExponentialBackoffBuilder::new()
        .with_initial_interval(Duration::from_millis(500))
        .with_multiplier(2.0)
        .with_randomization_factor(0.3)
        .with_max_interval(Duration::from_secs(30))
        .with_max_elapsed_time(Some(Duration::from_secs(90)))
        .build()
}

/// Send the request via `client`, retrying transient failures.
///
/// `build_request` is called for every attempt — it must rebuild the
/// `RequestBuilder` each time because `reqwest::blocking::RequestBuilder`
/// is consumed by `send()`. The closure typically clones or recomputes
/// the body.
pub(super) fn send_with_retry<F>(
    _client: &Client,
    mut build_request: F,
    provider: &'static str,
) -> Result<Response, Error>
where
    F: FnMut() -> RequestBuilder,
{
    let op = || -> Result<Response, backoff::Error<Error>> {
        let resp = build_request().send().map_err(|e| {
            // `reqwest::Error` is generally transient — timeouts, connection
            // errors, DNS — except for builder validation errors which we
            // surface as permanent.
            if e.is_builder() {
                backoff::Error::permanent(Error::Http(format!("{provider} request build: {e}")))
            } else {
                backoff::Error::transient(Error::Http(format!("{provider} send: {e}")))
            }
        })?;

        let status = resp.status();
        match classify(status) {
            Class::Success => Ok(resp),
            Class::PermanentClient => Err(backoff::Error::permanent(Error::Http(format!(
                "{provider} returned {status}"
            )))),
            Class::Transient => {
                let retry_after = parse_retry_after(&resp);
                if let Some(d) = retry_after {
                    Err(backoff::Error::retry_after(
                        Error::Http(format!("{provider} returned {status} (Retry-After {d:?})")),
                        d,
                    ))
                } else {
                    Err(backoff::Error::transient(Error::Http(format!(
                        "{provider} returned {status}"
                    ))))
                }
            }
        }
    };

    backoff_retry(default_schedule(), op).map_err(|e| match e {
        backoff::Error::Permanent(inner) => inner,
        backoff::Error::Transient { err, .. } => err,
    })
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

/// Parse the `Retry-After` header.
///
/// Per RFC 7231, the value is either an HTTP-date (which we don't try
/// to decode here) or a delta-seconds integer. We accept the integer
/// form and ignore date forms.
fn parse_retry_after(resp: &Response) -> Option<Duration> {
    let hv = resp.headers().get(reqwest::header::RETRY_AFTER)?;
    let s = hv.to_str().ok()?;
    let n: u64 = s.trim().parse().ok()?;
    Some(Duration::from_secs(n.min(60))) // cap to 60s to avoid pathological waits
}

// `format!` is in `alloc::format` for no_std builds; here, std is in scope.
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
    fn schedule_has_finite_cap() {
        let s = default_schedule();
        // At least the elapsed-time cap stops the loop.
        assert!(s.max_elapsed_time.is_some());
    }
}
