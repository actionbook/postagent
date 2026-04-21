use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum AppError {
    #[error("Failed to connect to postagent server.")]
    ConnectionFailed,

    #[error("{0}")]
    ApiError(String),

    #[error("Auth not found for \"{site}\". Run: postagent auth {site}")]
    AuthNotFound { site: String },

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Http(#[from] reqwest::Error),

    #[error("{0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Error: API key cannot be empty.")]
    EmptyApiKey,

    #[error("Error: Permission denied. Check directory permissions.")]
    PermissionDenied,

    #[error("Aborted.")]
    Aborted,

    #[error("HTTP {status} {status_text}\n{body}")]
    HttpStatus {
        status: u16,
        status_text: String,
        body: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_failed_message() {
        let err = AppError::ConnectionFailed;
        assert_eq!(err.to_string(), "Failed to connect to postagent server.");
    }

    #[test]
    fn api_error_message() {
        let err = AppError::ApiError("rate limit exceeded".to_string());
        assert_eq!(err.to_string(), "rate limit exceeded");
    }

    #[test]
    fn auth_not_found_message() {
        let err = AppError::AuthNotFound {
            site: "github".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Auth not found for \"github\". Run: postagent auth github"
        );
    }

    #[test]
    fn invalid_url_message() {
        let err = AppError::InvalidUrl("not-a-url".to_string());
        assert_eq!(err.to_string(), "Invalid URL: not-a-url");
    }

    #[test]
    fn empty_api_key_message() {
        let err = AppError::EmptyApiKey;
        assert_eq!(err.to_string(), "Error: API key cannot be empty.");
    }

    #[test]
    fn permission_denied_message() {
        let err = AppError::PermissionDenied;
        assert_eq!(
            err.to_string(),
            "Error: Permission denied. Check directory permissions."
        );
    }

    #[test]
    fn aborted_message() {
        let err = AppError::Aborted;
        assert_eq!(err.to_string(), "Aborted.");
    }

    #[test]
    fn http_status_message() {
        let err = AppError::HttpStatus {
            status: 404,
            status_text: "Not Found".to_string(),
            body: "resource not found".to_string(),
        };
        assert_eq!(err.to_string(), "HTTP 404 Not Found\nresource not found");
    }
}
