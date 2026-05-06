use serde::{Deserialize, Serialize};
use wee_events::{
    CborDecoder, CborEncoder, DecodeError, EventData, EventDecoder, EventDecoders, EventEncoder,
    JsonDecoder, JsonEncoder,
};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Payload {
    amount: i64,
}

#[test]
fn json_encoder_and_decoder_round_trip_event_data() {
    let data = JsonEncoder
        .serialize(&Payload { amount: 7 })
        .expect("json encode should succeed");

    assert_eq!(data.encoding, EventData::JSON_ENCODING);

    let decoded: Payload = JsonDecoder
        .deserialize(&data)
        .expect("json decode should succeed");
    assert_eq!(decoded, Payload { amount: 7 });
}

#[test]
fn cbor_encoder_and_decoder_round_trip_event_data() {
    let data = CborEncoder
        .serialize(&Payload { amount: 9 })
        .expect("cbor encode should succeed");

    assert_eq!(data.encoding, CborEncoder::ENCODING);
    assert_ne!(data.encoding, EventData::JSON_ENCODING);

    let decoded: Payload = CborDecoder
        .deserialize(&data)
        .expect("cbor decode should succeed");
    assert_eq!(decoded, Payload { amount: 9 });
}

#[test]
fn decoder_set_selects_decoder_by_event_data_encoding() {
    let decoders = EventDecoders::new().with(JsonDecoder).with(CborDecoder);
    let data = CborEncoder
        .serialize(&Payload { amount: 11 })
        .expect("cbor encode should succeed");

    let decoded: Payload = decoders
        .deserialize(&data)
        .expect("decoder set should find cbor decoder");

    assert_eq!(decoded, Payload { amount: 11 });
}

#[test]
fn decoder_set_reports_unknown_encoding() {
    let decoders = EventDecoders::new().with(JsonDecoder);
    let data = EventData::raw("application/x-custom", vec![1, 2, 3]);

    let error = decoders
        .deserialize::<Payload>(&data)
        .expect_err("unknown encoding should fail");

    assert!(matches!(
        error,
        DecodeError::UnknownEncoding { encoding } if encoding == "application/x-custom"
    ));
}

#[test]
fn event_data_json_helpers_preserve_existing_behavior() {
    let data = EventData::json(&Payload { amount: 13 }).expect("json helper should encode");
    let decoded: Payload = data.deserialize_json().expect("json helper should decode");

    assert_eq!(decoded, Payload { amount: 13 });
}
