use serde::{Deserialize, Serialize};
use wee_events::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Increment {
    pub amount: i64,
}

impl Command for Increment {
    const NAME: &'static str = "counter:increment";
}
