//! HTTPS requests as Android's `Bridge` makes them, one hop at a time (SEC-01, SEC-02,
//! ADR 0027).
//!
//! `Bridge.httpsGet(url, maxBytes, outcome)` makes one request and never follows a redirect
//! itself. It returns the body, or `null`, and fills `outcome` with a kind and a detail: one of
//! the kinds below, which must stay in step with `Bridge.java`. [`fetch`] decides which
//! redirects to follow, so the policy is tested without a device: only to `https://`, and at
//! most [`MAX_REDIRECTS`] of them.

use crate::model::DataError;
use crate::problem::Problem;

/// How many redirects one request may follow before it fails.
pub(crate) const MAX_REDIRECTS: usize = 3;

/// The body follows; the detail is empty.
const OK: &str = "ok";
/// No body; the detail is where the request redirects, as an absolute URL.
const REDIRECT: &str = "redirect";
/// No connection.
const OFFLINE: &str = "offline";
/// The body is larger than `maxBytes`.
const TOO_LARGE: &str = "too-large";
/// The URL isn't `https:`, or the platform refused cleartext traffic.
const NOT_HTTPS: &str = "not-https";
/// The secure connection failed, for example its certificate didn't validate.
const INSECURE: &str = "insecure";
// Any other kind, such as "failed", is a failure.

/// What one request made.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Hop {
    Body(Vec<u8>),
    /// Where to go instead, as an absolute URL.
    Redirect(String),
}

/// Fetches `url`, asking `hop` for each request, and follows at most [`MAX_REDIRECTS`]
/// redirects, each only to `https://`. A URL that isn't `https://` is never requested, so a
/// redirect to plain HTTP fails without reaching it (SEC-01).
pub(crate) fn fetch(
    url: &str,
    max_bytes: usize,
    mut hop: impl FnMut(&str) -> Result<Hop, DataError>,
) -> Result<Vec<u8>, DataError> {
    let mut url = url.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        if !is_https(&url) {
            return Err(not_https());
        }
        match hop(&url)? {
            Hop::Body(body) if body.len() > max_bytes => return Err(too_large()),
            Hop::Body(body) => return Ok(body),
            Hop::Redirect(location) => url = location,
        }
    }
    Err(DataError::Failed(Problem::TooManyRedirects))
}

/// The hop the bridge reports: `body` and its `[kind, detail]` outcome.
pub(crate) fn hop(body: Option<Vec<u8>>, outcome: &[Option<String>]) -> Result<Hop, DataError> {
    let field = |index: usize| outcome.get(index).cloned().flatten();
    match (field(0).as_deref(), body) {
        (Some(OK), Some(body)) => Ok(Hop::Body(body)),
        (Some(REDIRECT), _) => field(1)
            .filter(|location| !location.is_empty())
            .map(Hop::Redirect)
            .ok_or_else(failed),
        (Some(OFFLINE), _) => Err(DataError::Offline),
        (Some(TOO_LARGE), _) => Err(too_large()),
        (Some(NOT_HTTPS), _) => Err(not_https()),
        (Some(INSECURE), _) => Err(DataError::Failed(Problem::ConnectionInsecure)),
        _ => Err(failed()),
    }
}

/// The scheme is case-insensitive (RFC 3986), and something must follow it.
fn is_https(url: &str) -> bool {
    url.len() > "https://".len()
        && url
            .get(.."https://".len())
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

fn not_https() -> DataError {
    DataError::Invalid(Problem::HttpsOnly)
}

fn too_large() -> DataError {
    DataError::Invalid(Problem::DownloadTooLarge)
}

fn failed() -> DataError {
    DataError::Failed(Problem::DownloadFailed)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    const FEED: &str = "https://www.example.com/feed";

    /// Answers each request with the next of its hops, and records every URL requested.
    struct Server {
        hops: VecDeque<Result<Hop, DataError>>,
        requested: Vec<String>,
    }

    impl Server {
        fn new(hops: impl IntoIterator<Item = Result<Hop, DataError>>) -> Self {
            Self {
                hops: hops.into_iter().collect(),
                requested: Vec::new(),
            }
        }

        fn fetch(&mut self, url: &str) -> Result<Vec<u8>, DataError> {
            fetch(url, 100, |url| {
                self.requested.push(url.to_owned());
                self.hops.pop_front().unwrap_or_else(|| Err(failed()))
            })
        }
    }

    fn outcome(kind: &str, detail: Option<&str>) -> Vec<Option<String>> {
        vec![Some(kind.to_owned()), detail.map(str::to_owned)]
    }

    #[test]
    fn a_body_is_returned_after_one_request() {
        let mut server = Server::new([Ok(Hop::Body(b"{}".to_vec()))]);

        assert_eq!(server.fetch(FEED), Ok(b"{}".to_vec()));
        assert_eq!(server.requested, [FEED]);
    }

    #[test]
    fn a_redirect_to_plain_http_fails_without_requesting_it() {
        // SEC-01; issue #26: "Given a redirect to a plain-HTTP URL, then the request fails."
        let mut server = Server::new([
            Ok(Hop::Redirect("http://www.example.com/feed".into())),
            Ok(Hop::Body(b"{}".to_vec())),
        ]);

        assert_eq!(
            server.fetch(FEED),
            Err(DataError::Invalid(Problem::HttpsOnly))
        );
        assert_eq!(server.requested, [FEED]);
    }

    #[test]
    fn a_plain_http_url_is_never_requested() {
        // SEC-01
        for url in [
            "http://www.example.com/feed",
            "ftp://example.com",
            "https://",
            "",
        ] {
            let mut server = Server::new([Ok(Hop::Body(Vec::new()))]);

            assert_eq!(
                server.fetch(url),
                Err(DataError::Invalid(Problem::HttpsOnly)),
                "{url}"
            );
            assert!(server.requested.is_empty());
        }
    }

    #[test]
    fn https_redirects_are_followed_up_to_the_limit() {
        let moved = "HTTPS://cdn.example.com/feed";
        let mut server = Server::new([
            Ok(Hop::Redirect(moved.into())),
            Ok(Hop::Body(b"moved".to_vec())),
        ]);
        assert_eq!(server.fetch(FEED), Ok(b"moved".to_vec()));
        assert_eq!(server.requested, [FEED, moved]);

        let mut endless = Server::new(
            (0..=MAX_REDIRECTS).map(|n| Ok(Hop::Redirect(format!("https://example.com/{n}")))),
        );
        assert_eq!(
            endless.fetch(FEED),
            Err(DataError::Failed(Problem::TooManyRedirects))
        );
        assert_eq!(endless.requested.len(), MAX_REDIRECTS + 1);
    }

    #[test]
    fn a_body_larger_than_allowed_is_rejected() {
        // SEC-02
        let mut server = Server::new([Ok(Hop::Body(vec![0; 101]))]);

        assert_eq!(
            server.fetch(FEED),
            Err(DataError::Invalid(Problem::DownloadTooLarge))
        );
    }

    #[test]
    fn a_failed_request_is_not_retried() {
        // REL-04: one request per call; retrying is the updater's decision.
        let mut server = Server::new([Err(DataError::Offline), Ok(Hop::Body(Vec::new()))]);

        assert_eq!(server.fetch(FEED), Err(DataError::Offline));
        assert_eq!(server.requested.len(), 1);
    }

    #[test]
    fn each_outcome_the_bridge_reports_has_its_meaning() {
        let body = || Some(b"x".to_vec());
        assert_eq!(
            hop(body(), &outcome(OK, None)),
            Ok(Hop::Body(b"x".to_vec()))
        );
        assert_eq!(
            hop(None, &outcome(REDIRECT, Some("https://example.com/b"))),
            Ok(Hop::Redirect("https://example.com/b".into()))
        );
        assert_eq!(hop(None, &outcome(OFFLINE, None)), Err(DataError::Offline));
        assert_eq!(
            hop(None, &outcome(TOO_LARGE, None)),
            Err(DataError::Invalid(Problem::DownloadTooLarge))
        );
        assert_eq!(
            hop(None, &outcome(NOT_HTTPS, None)),
            Err(DataError::Invalid(Problem::HttpsOnly))
        );
        assert_eq!(
            hop(None, &outcome(INSECURE, None)),
            Err(DataError::Failed(Problem::ConnectionInsecure))
        );
    }

    #[test]
    fn an_outcome_that_doesnt_make_sense_is_a_failure() {
        let failed = Err(DataError::Failed(Problem::DownloadFailed));
        for (body, outcome) in [
            (None, outcome(OK, None)),
            (None, outcome(REDIRECT, None)),
            (None, outcome(REDIRECT, Some(""))),
            (Some(Vec::new()), outcome("failed", Some("404"))),
            (Some(Vec::new()), outcome("something new", None)),
            (Some(Vec::new()), vec![None, None]),
            (Some(Vec::new()), Vec::new()),
        ] {
            assert_eq!(hop(body, &outcome), failed, "{outcome:?}");
        }
    }
}
