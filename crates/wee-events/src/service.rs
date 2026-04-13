/// A structured rejection from the domain layer. Indicates a command was
/// refused by business logic (as opposed to an infrastructure failure).
///
/// Carries a machine-readable code, human message, and arbitrary JSON context.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct Rejection {
    pub code: String,
    pub message: String,
    #[serde(default = "default_context")]
    pub context: serde_json::Value,
}

fn default_context() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

impl Rejection {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            context: default_context(),
        }
    }

    pub fn with_context(
        code: impl Into<String>,
        message: impl Into<String>,
        context: serde_json::Value,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            context,
        }
    }
}
