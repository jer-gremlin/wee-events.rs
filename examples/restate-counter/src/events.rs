use serde::{Deserialize, Serialize};
use wee_events::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, DomainEvent)]
#[domain_event(prefix = "counter")]
pub enum CounterEvent {
    Incremented { amount: i64 },
    Reset,
    Randomised { amount: i64 },
}
