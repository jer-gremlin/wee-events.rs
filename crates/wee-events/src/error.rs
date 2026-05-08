use std::fmt;

use crate::id::Revision;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryDiagnostics {
    pub last_attempted_revision: Revision,
    pub observed_max_revision: Revision,
    pub possible_clock_skew_ms: Option<u64>,
}

impl fmt::Display for RetryDiagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "last attempted revision {}, observed max revision {}",
            self.last_attempted_revision, self.observed_max_revision
        )?;
        if let Some(skew_ms) = self.possible_clock_skew_ms {
            write!(f, ", possible clock skew {} ms", skew_ms)?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("revision conflict: expected {expected}, found {actual}")]
    RevisionConflict {
        expected: Revision,
        actual: Revision,
    },

    #[error("encoding mismatch: expected {expected}, actual {actual}")]
    EncodingMismatch { expected: String, actual: String },

    #[error("unhandled event type: {event_type}")]
    UnhandledEventType { event_type: String },

    #[error("publish failed after {attempts} attempts ({diagnostics})")]
    RetryExhausted {
        attempts: usize,
        diagnostics: RetryDiagnostics,
    },
}

/// Extension trait for store error types to expose structural failures
/// uniformly across backends.
///
/// The conformance test suite is generic over `EventStore` and therefore sees
/// `Self::Error` rather than the structural `crate::Error` directly. Each
/// store implementation provides this view so portable assertions like
/// "this failure is a revision conflict" can be written without naming the
/// concrete store error type.
pub trait EventStoreErrorExt {
    /// Returns the underlying structural error if this error wraps one.
    fn as_wee_events(&self) -> Option<&Error>;
}

impl EventStoreErrorExt for Error {
    fn as_wee_events(&self) -> Option<&Error> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{RetryDiagnostics, Revision};

    #[test]
    fn retry_diagnostics_display_without_clock_skew_hint() {
        let diagnostics = RetryDiagnostics {
            last_attempted_revision: Revision::new("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            observed_max_revision: Revision::new("01ARZ3NDEKTSV4RRFFQ69G5FAW"),
            possible_clock_skew_ms: None,
        };

        assert_eq!(
            diagnostics.to_string(),
            "last attempted revision 01ARZ3NDEKTSV4RRFFQ69G5FAV, observed max revision 01ARZ3NDEKTSV4RRFFQ69G5FAW"
        );
    }

    #[test]
    fn retry_diagnostics_display_with_clock_skew_hint() {
        let diagnostics = RetryDiagnostics {
            last_attempted_revision: Revision::new("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            observed_max_revision: Revision::new("01ARZ3NDEKTSV4RRFFQ69G5FAW"),
            possible_clock_skew_ms: Some(17),
        };

        assert_eq!(
            diagnostics.to_string(),
            "last attempted revision 01ARZ3NDEKTSV4RRFFQ69G5FAV, observed max revision 01ARZ3NDEKTSV4RRFFQ69G5FAW, possible clock skew 17 ms"
        );
    }
}
