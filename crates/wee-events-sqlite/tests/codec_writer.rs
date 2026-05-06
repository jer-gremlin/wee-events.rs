use serde::{Deserialize, Serialize};
use wee_events::{
    AggregateId, CborDecoder, CborEncoder, DomainEvent, Entity, EventDecoders, EventEncoder,
    EventStore as _, JsonDecoder, JsonEncoder, Publisher, Revision,
};
use wee_events_sqlite::{GlobalStrategy, SqliteEventStore};

#[derive(Debug, Clone, Serialize, Deserialize, DomainEvent)]
#[domain_event(prefix = "counter")]
enum CounterEvent {
    Incremented { amount: i64 },
}

#[derive(Debug, Clone, Default)]
struct Counter;

fn entity() -> Entity<Counter> {
    Entity {
        aggregate_id: AggregateId::new("counter", "codec"),
        revision: Revision::zero(),
        state: Counter,
    }
}

#[tokio::test]
async fn sqlite_writer_encodes_typed_published_events_as_cbor() {
    let store = SqliteEventStore::builder()
        .in_memory()
        .strategy(GlobalStrategy)
        .writer(CborEncoder)
        .open()
        .await
        .expect("store should open");

    Publisher::new(&store)
        .publish(&entity(), vec![CounterEvent::Incremented { amount: 5 }])
        .await
        .expect("publish should succeed");

    let aggregate = store
        .load(&AggregateId::new("counter", "codec"))
        .await
        .expect("load should work");
    let recorded = &aggregate.events()[0];

    assert_eq!(recorded.data.encoding, CborEncoder::ENCODING);

    let decoders = EventDecoders::new().with(JsonDecoder).with(CborDecoder);
    let decoded: CounterEvent = decoders
        .deserialize(&recorded.data)
        .expect("consumer decoder should decode cbor");

    assert!(matches!(decoded, CounterEvent::Incremented { amount: 5 }));
}

#[tokio::test]
async fn sqlite_writer_can_remain_json_for_compatibility() {
    let store = SqliteEventStore::builder()
        .in_memory()
        .strategy(GlobalStrategy)
        .writer(JsonEncoder)
        .open()
        .await
        .expect("store should open");

    Publisher::new(&store)
        .publish(&entity(), vec![CounterEvent::Incremented { amount: 3 }])
        .await
        .expect("publish should succeed");

    let aggregate = store
        .load(&AggregateId::new("counter", "codec"))
        .await
        .expect("load should work");
    assert_eq!(
        aggregate.events()[0].data.encoding,
        wee_events::EventData::JSON_ENCODING
    );
}
