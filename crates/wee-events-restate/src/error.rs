use wee_events::{CodecError, DecodeError, EncodeError, Rejection, ServiceError};

/// Errors produced by the wee-events-restate adapter.
///
/// Distinguishes transport failures, JSON decode failures, backend HTTP
/// error bodies that aren't structured rejections, domain rejections from
/// remote services, and structural store failures from the wee-events core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("decode: {0}")]
    Decode(#[from] DecodeError),

    #[error("encode: {0}")]
    Encode(#[from] EncodeError),

    #[error("backend: {0}")]
    Backend(String),

    #[error(transparent)]
    Rejection(Rejection),

    #[error(transparent)]
    Store(#[from] wee_events::Error),
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Decode(DecodeError::Json(err))
    }
}

impl From<CodecError> for Error {
    fn from(err: CodecError) -> Self {
        match err {
            CodecError::Encode(e) => Error::Encode(e),
            CodecError::Decode(e) => Error::Decode(e),
        }
    }
}

/// Lifts a service-layer error into the adapter error.
///
/// Rejections and codec failures map cleanly onto adapter variants. Generic
/// store failures collapse to `Backend(Display)` because adapter error types
/// aren't generic over the underlying store error.
impl<E> From<ServiceError<E>> for Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn from(value: ServiceError<E>) -> Self {
        match value {
            ServiceError::Rejection(r) => Error::Rejection(r),
            ServiceError::Codec(e) => e.into(),
            ServiceError::Store(e) => Error::Backend(e.to_string()),
        }
    }
}
