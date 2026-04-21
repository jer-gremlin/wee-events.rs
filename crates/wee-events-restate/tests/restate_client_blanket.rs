use wee_events::{Command, HasCommand, Handles, ServiceDefinition, TypedService};
use wee_events_restate::RestateClient;

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Counter;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Inc;
impl Command for Inc {
    const NAME: &'static str = "c:inc";
}

pub struct Svc;
impl ServiceDefinition for Svc {
    type State = Counter;
    const SERVICE_NAME: &'static str = "c";
}
impl HasCommand<Inc> for Svc {}

#[test]
fn restate_client_satisfies_typed_service_and_handles() {
    fn require<T: TypedService<Counter> + Handles<Inc>>() {}
    require::<RestateClient<Svc>>();
}
