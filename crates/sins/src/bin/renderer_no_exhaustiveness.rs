use serde::{Deserialize, Serialize};
use wee_events::{
    Aggregate, AggregateId, DomainEvent, EventData, EventId, EventMetadata, EventType,
    RecordedEvent, Renderer, Revision,
};

#[derive(Debug, Serialize, Deserialize, DomainEvent)]
#[domain_event(prefix = "door")]
enum DoorEvent {
    Opened,
    Locked, // unwired below
}

#[derive(Debug, Default)]
struct State;

fn on_opened(_: &mut State, _: &EventType, _: &EventData) -> Result<(), serde_json::Error> {
    Ok(())
}

fn rec(t: &str, rev: u32) -> RecordedEvent {
    RecordedEvent {
        event_id: EventId::new(format!("e{rev}")),
        event_type: EventType::new(t),
        revision: Revision::new(format!("{rev:026}")), //NOT GOOD see the `revision_lies` sinbin
        metadata: EventMetadata::default(),
        data: EventData::raw("application/json", b"null".to_vec()),
    }
}

fn main() {
    // Renderer compiled green despite missing `Locked` reducer.
    let r: Renderer<State, serde_json::Error> = Renderer::new().with(DoorEvent::OPENED, on_opened);
    // some kinda <X: EnsuresExhaustic> could maybe be a good trait-bound on the Renderer::with<X... impl

    let agg = Aggregate::from_events(
        AggregateId::new("door", "front"),
        vec![rec(DoorEvent::OPENED, 1), rec(DoorEvent::LOCKED, 2)],
    );

    let result = r.render(&agg);
    assert!(
        result.is_ok(),
        "Renderer compiled despite missing variant coverage; runtime: {result:?}",
    );
    /* SUGGESTION:
    I don't have a great fix in mind for this... you could do some kinda renderer! macro, and force the match arms on the event to be exhaustive..

    The sinbin here probably makes it seem a little easier to abuse than a _real_ problem.

    */
}
