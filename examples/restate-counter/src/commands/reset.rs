use serde::{Deserialize, Serialize};
use wee_events::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reset;

impl Command for Reset {
    const NAME: &'static str = "counter:reset";
}
