use wee_events::{Command, HasCommand, ServiceDefinition};

struct Counter;
struct DemoService;

impl ServiceDefinition for DemoService {
    type State = Counter;
    const SERVICE_NAME: &'static str = "demo";
}

#[test]
fn service_definition_is_object_safe_ish() {
    assert_eq!(<DemoService as ServiceDefinition>::SERVICE_NAME, "demo");
}

#[derive(serde::Serialize)]
struct Bump;
impl Command for Bump {
    const NAME: &'static str = "demo:bump";
}

impl HasCommand<Bump> for DemoService {}

#[test]
fn has_command_impl_resolves() {
    fn require<T: HasCommand<Bump>>() {}
    require::<DemoService>();
}
