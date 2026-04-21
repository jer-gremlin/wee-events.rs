use wee_events::ServiceDefinition;

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
