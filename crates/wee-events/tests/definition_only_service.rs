#![allow(dead_code)]

use wee_events::{Command, HasCommand, ServiceDefinition};

#[derive(Default)]
pub struct Counter {
    value: i64,
}

#[derive(serde::Serialize)]
struct Increment;
impl Command for Increment {
    const NAME: &'static str = "counter:increment";
}

#[derive(serde::Serialize)]
struct Reset;
impl Command for Reset {
    const NAME: &'static str = "counter:reset";
}

wee_events::service! {
    pub CounterService("counter") for Counter [Increment, Reset]
}

#[test]
fn definition_only_emits_definition_traits() {
    fn require_def<D: ServiceDefinition>() {}
    fn require_has<D: HasCommand<Increment> + HasCommand<Reset>>() {}

    require_def::<CounterService>();
    require_has::<CounterService>();
    assert_eq!(CounterService::SERVICE_NAME, "counter");
}
