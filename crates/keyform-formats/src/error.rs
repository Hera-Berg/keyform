use thiserror::Error;

/// Errors from reading or writing any Keyform file format.
///
/// Every input is treated as untrusted: malformed data produces one of these,
/// never a panic.
#[derive(Debug, Error)]
pub enum FormatError {
    #[error("unexpected end of data at offset {offset} (needed {needed} bytes)")]
    UnexpectedEof { offset: usize, needed: usize },
    #[error("bad magic: expected {expected}")]
    BadMagic { expected: &'static str },
    #[error("invalid data: {0}")]
    Invalid(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("xml: {0}")]
    Xml(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, FormatError>;

pub(crate) fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(FormatError::Invalid(msg.into()))
}
