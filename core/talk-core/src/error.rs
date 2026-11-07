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
}

pub type Result<T> = std::result::Result<T, Error>;
