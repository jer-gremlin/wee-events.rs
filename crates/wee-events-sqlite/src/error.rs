#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("libsql error: {0}")]
    Libsql(#[from] libsql::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    WeeEvents(#[from] wee_events::Error),
}

impl wee_events::EventStoreErrorExt for Error {
    fn as_wee_events(&self) -> Option<&wee_events::Error> {
        match self {
            Error::WeeEvents(e) => Some(e),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use wee_events::{RetryDiagnostics, Revision};

    #[test]
    fn sqlite_serialization_errors_display_includes_inner_message() {
        let sqlite_error = Error::Serialization(
            serde_json::from_str::<serde_json::Value>("{invalid json")
                .expect_err("invalid JSON should fail"),
        );

        let rendered = sqlite_error.to_string();
        assert!(
            rendered.starts_with("serialization error: "),
            "unexpected display: {rendered}"
        );
    }

    #[test]
    fn sqlite_retry_exhausted_errors_preserve_retry_diagnostics() {
        let diagnostics = RetryDiagnostics {
            last_attempted_revision: Revision::new("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            observed_max_revision: Revision::new("01ARZ3NDEKTSV4RRFFQ69G5FAW"),
            possible_clock_skew_ms: Some(17),
        };

        let error = Error::WeeEvents(wee_events::Error::RetryExhausted {
            attempts: 5,
            diagnostics: diagnostics.clone(),
        });

        match error {
            Error::WeeEvents(wee_events::Error::RetryExhausted {
                attempts,
                diagnostics: actual,
            }) => {
                assert_eq!(attempts, 5);
                assert_eq!(actual, diagnostics);
            }
            other => panic!("expected RetryExhausted, got {other}"),
        }
    }
}
