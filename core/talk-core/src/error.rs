use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("unauthorized – app password revoked or wrong credentials")]
    Unauthorized,

    #[error("not found")]
    NotFound,

    #[error("server returned HTTP {status}: {message}")]
    Http { status: u16, message: String },

    #[error("unexpected response: {0}")]
    Decode(String),

    #[error("login flow expired before it was approved")]
    LoginExpired,

    /// HTTP 429/503: the server asks to come back later (`Retry-After` seconds if given).
    #[error("server busy (HTTP {status}), retry later")]
    RetryLater { status: u16, after_secs: Option<u64> },

    /// A local file to upload is missing or unreadable.
    #[error("local file: {0}")]
    LocalFile(String),
}

impl Error {
    /// Whether the request may have reached the server although it failed (timeouts,
    /// connection resets after sending). Only a failed connect proves it did not.
    pub fn maybe_delivered(&self) -> bool {
        match self {
            Error::Network(e) => !e.is_connect() && !e.is_builder(),
            Error::Decode(_) => true,
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
