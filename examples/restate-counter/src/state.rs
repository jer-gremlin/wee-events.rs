use serde::{Deserialize, Serialize};
use wee_events::{
    CborDecoder, DecodeError, EventData, EventDecoders, EventType, JsonDecoder, Renderer,
};

use crate::events::CounterEvent;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Counter {
    pub value: i64,
    pub randomised_count: u64,
}

pub fn renderer() -> Renderer<Counter, DecodeError> {
    Renderer::new()
        .with(CounterEvent::INCREMENTED, reduce_incremented)
        .with(CounterEvent::RESET, reduce_reset)
        .with(CounterEvent::RANDOMISED, reduce_randomised)
}

fn decode_event(data: &EventData) -> Result<CounterEvent, DecodeError> {
    EventDecoders::new()
        .with(JsonDecoder)
        .with(CborDecoder)
        .deserialize(data)
}

fn reduce_incremented(
    state: &mut Counter,
    event_type: &EventType,
    data: &EventData,
) -> Result<(), DecodeError> {
    let amount = match decode_event(data)? {
        CounterEvent::Incremented { amount } => amount,
        _ => return unexpected_payload(event_type),
    };
    state.value += amount;
    Ok(())
}

fn reduce_reset(
    state: &mut Counter,
    _event_type: &EventType,
    _data: &EventData,
) -> Result<(), DecodeError> {
    state.value = 0;
    Ok(())
}

fn reduce_randomised(
    state: &mut Counter,
    event_type: &EventType,
    data: &EventData,
) -> Result<(), DecodeError> {
    let amount = match decode_event(data)? {
        CounterEvent::Randomised { amount } => amount,
        _ => return unexpected_payload(event_type),
    };
    state.value += amount;
    state.randomised_count += 1;
    Ok(())
}

fn unexpected_payload<T>(event_type: &EventType) -> Result<T, DecodeError> {
    Err(DecodeError::InvalidData {
        message: format!("payload did not decode as {event_type}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wee_events::{CborEncoder, EventEncoder};

    #[test]
    fn reducer_reports_unexpected_payload_as_decode_error() {
        let data = CborEncoder
            .serialize(&CounterEvent::Randomised { amount: 7 })
            .expect("event should encode");
        let mut state = Counter::default();

        let error = reduce_incremented(
            &mut state,
            &EventType::new(CounterEvent::INCREMENTED),
            &data,
        )
        .expect_err("unexpected decoded payload should fail");

        assert!(matches!(
            error,
            DecodeError::InvalidData { message }
                if message.contains(CounterEvent::INCREMENTED)
        ));
    }
}
