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

    #[error("publish failed after {attempts} attempts ({diagnostics})")]
    RetryExhausted {
        attempts: usize,
        diagnostics: RetryDiagnostics,
    },
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
