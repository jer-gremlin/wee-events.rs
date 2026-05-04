use serde::{Deserialize, Serialize};
use wee_events::{EventData, EventType, Renderer};

use crate::events::CounterEvent;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Counter {
    pub value: i64,
    pub randomised_count: u64,
}

pub fn renderer() -> Renderer<Counter> {
    Renderer::new()
        .with(CounterEvent::INCREMENTED, reduce_incremented)
        .with(CounterEvent::RESET, reduce_reset)
        .with(CounterEvent::RANDOMISED, reduce_randomised)
}

fn reduce_incremented(
    state: &mut Counter,
    _event_type: &EventType,
    data: &EventData,
) -> wee_events::Result<()> {
    let CounterEvent::Incremented { amount } = data.deserialize_json()? else {
        return Ok(());
    };
    state.value += amount;
    Ok(())
}

fn reduce_reset(
    state: &mut Counter,
    _event_type: &EventType,
    _data: &EventData,
) -> wee_events::Result<()> {
    state.value = 0;
    Ok(())
}

fn reduce_randomised(
    state: &mut Counter,
    _event_type: &EventType,
    data: &EventData,
) -> wee_events::Result<()> {
    let CounterEvent::Randomised { amount } = data.deserialize_json()? else {
        return Ok(());
    };
    state.value += amount;
    state.randomised_count += 1;
    Ok(())
}
