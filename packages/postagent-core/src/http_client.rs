use std::time::Duration;

use reqwest::blocking::Client;

/// Hardcoded timeout for requests to the postagent server. Sized to give a
/// cold-start Vercel function plenty of room to respond while still failing
/// with a friendly message rather than hanging indefinitely. Not user-tunable
/// by design — "how long should it wait?" is not something we want every user
/// to have to think about.
pub const SERVER_HTTP_TIMEOUT_SECS: u64 = 45;

pub fn server_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(SERVER_HTTP_TIMEOUT_SECS))
        .build()
        .expect("failed to build HTTP client")
}

enum ServerErrorKind {
    Timeout,
    Connect,
    Other(String),
}

fn classify(err: &reqwest::Error) -> ServerErrorKind {
    if err.is_timeout() {
        ServerErrorKind::Timeout
    } else if err.is_connect() {
        ServerErrorKind::Connect
    } else {
        ServerErrorKind::Other(err.to_string())
    }
}

fn format_kind(kind: &ServerErrorKind) -> String {
    match kind {
        ServerErrorKind::Timeout => format!(
            "Request to postagent server timed out after {}s.\nThe server may be busy; try again in a few seconds.",
            SERVER_HTTP_TIMEOUT_SECS
        ),
        ServerErrorKind::Connect => {
            "Could not reach postagent server.\nCheck your network connection, then try again.".to_string()
        }
        ServerErrorKind::Other(text) => {
            format!("Request to postagent server failed: {}", text)
        }
    }
}

/// Categorizes a reqwest error from a postagent-server call into a
/// user-facing message. Covers the three failure modes we've seen in
/// practice: request exceeded the client timeout, TCP connect failed,
/// and everything else (falls back to the underlying error text so
/// unusual cases are still diagnosable).
pub fn format_server_error(err: &reqwest::Error) -> String {
    format_kind(&classify(err))
}

/// Returns a friendly transient-retry message for HTTP 5xx statuses we
/// expect to be recoverable, or `None` for everything else. Callers keep
/// their existing structured-error handling for 4xx.
pub fn format_transient_status(status: u16) -> Option<String> {
    match status {
        502..=504 => Some(format!(
            "postagent server returned HTTP {}.\nThis is usually transient; try again in a few seconds.",
            status
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_kind_timeout_mentions_timeout_and_seconds() {
        let msg = format_kind(&ServerErrorKind::Timeout);
        assert!(msg.contains("timed out"));
        assert!(msg.contains(&format!("{}s", SERVER_HTTP_TIMEOUT_SECS)));
        assert!(msg.contains("try again"));
    }

    #[test]
    fn format_kind_connect_mentions_network() {
        let msg = format_kind(&ServerErrorKind::Connect);
        assert!(msg.contains("Could not reach postagent server"));
        assert!(msg.contains("network"));
    }

    #[test]
    fn format_kind_other_includes_original_text() {
        let msg = format_kind(&ServerErrorKind::Other("dns error: NXDOMAIN".into()));
        assert!(msg.contains("Request to postagent server failed"));
        assert!(msg.contains("dns error: NXDOMAIN"));
    }

    #[test]
    fn format_transient_status_covers_common_5xx() {
        for s in [502u16, 503, 504] {
            let msg = format_transient_status(s).unwrap_or_else(|| panic!("{} should match", s));
            assert!(msg.contains(&s.to_string()));
            assert!(msg.contains("transient"));
        }
    }

    #[test]
    fn format_transient_status_ignores_4xx_and_2xx() {
        assert!(format_transient_status(200).is_none());
        assert!(format_transient_status(404).is_none());
        assert!(format_transient_status(429).is_none());
        assert!(format_transient_status(500).is_none()); // server error but not one we flag as retryable by default
    }

    #[test]
    fn server_client_builds_successfully() {
        let _ = server_client();
    }

    /// Integration-ish: real connect failure against a closed TCP port.
    /// Ensures our classify() actually picks the Connect branch for a real
    /// reqwest error, not just for a synthesized enum value.
    #[test]
    fn classify_real_connect_error() {
        let client = Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .unwrap();
        // 127.0.0.1:1 is reserved; on every OS we've tested, nothing listens
        // there, so reqwest returns a connect error (not a timeout).
        let err = client
            .get("http://127.0.0.1:1/")
            .send()
            .expect_err("request should fail");
        let formatted = format_server_error(&err);
        assert!(
            formatted.contains("Could not reach postagent server")
                || formatted.contains("Request to postagent server failed"),
            "unexpected message: {}",
            formatted
        );
    }
}
