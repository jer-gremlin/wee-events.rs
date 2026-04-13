use wee_events::Rejection;

#[test]
fn rejection_displays_code_and_message() {
    let r = Rejection::new("INSUFFICIENT_FUNDS", "balance too low");
    assert_eq!(r.code, "INSUFFICIENT_FUNDS");
    assert_eq!(r.message, "balance too low");
    assert_eq!(r.to_string(), "INSUFFICIENT_FUNDS: balance too low");
}

#[test]
fn rejection_carries_context() {
    let r = Rejection::with_context(
        "LIMIT_EXCEEDED",
        "over the limit",
        serde_json::json!({ "limit": 100, "actual": 150 }),
    );
    assert_eq!(r.context["actual"], 150);
}

#[test]
fn rejection_default_context_is_empty_object() {
    let r = Rejection::new("CODE", "msg");
    assert_eq!(r.context, serde_json::json!({}));
}

#[test]
fn rejection_converts_to_error() {
    let r = Rejection::new("CODE", "msg");
    let err: wee_events::Error = r.into();
    assert!(matches!(err, wee_events::Error::Rejection(_)));
}
