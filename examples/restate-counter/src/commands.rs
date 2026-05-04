use serde::{Deserialize, Serialize};
use wee_events::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Increment {
    pub amount: i64,
}

impl Command for Increment {
    const NAME: &'static str = "counter:increment";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reset;

impl Command for Reset {
    const NAME: &'static str = "counter:reset";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Randomise {
    pub min: i64,
    pub max: i64,
}

impl Command for Randomise {
    const NAME: &'static str = "counter:randomise";
}
