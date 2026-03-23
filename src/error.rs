use thiserror::Error;

#[derive(Debug, Error)]
pub enum TcgError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Failed to parse HTML: {0}")]
    Parse(String),

    #[error("Missing expected element: {0}")]
    MissingElement(String),

    #[error("JSON deserialization failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Product not found: {0}")]
    NotFound(u64),

    #[error("Rate limited (HTTP {0})")]
    RateLimited(u16),
}

pub type Result<T> = std::result::Result<T, TcgError>;
