use std::future::Future;
use wee_events::{AggregateId, Command, CommandName, Entity, Handles, Revision, TypedService};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
struct Increment {
    amount: i64,
}

impl Command for Increment {
    fn command_name(&self) -> CommandName {
        CommandName::from("counter:increment")
    }
}

#[derive(Debug, Clone, Serialize)]
struct Unsupported;

impl Command for Unsupported {
    fn command_name(&self) -> CommandName {
        CommandName::from("counter:unsupported")
    }
}

#[derive(Debug, Default, Clone)]
struct Counter {
    value: i64,
}

struct TestService;

impl Handles<Increment> for TestService {}

impl TypedService<Counter> for TestService {
    fn load(
        &self,
        id: &AggregateId,
    ) -> impl Future<Output = wee_events::Result<Entity<Counter>>> + Send {
        let id = id.clone();
        async move {
            Ok(Entity {
                aggregate_id: id,
                revision: Revision::zero(),
                state: Counter { value: 0 },
            })
        }
    }

    fn execute<C, Idx>(
        &self,
        id: &AggregateId,
        _cmd: C,
    ) -> impl Future<Output = wee_events::Result<Entity<Counter>>> + Send
    where
        C: Command + Serialize + Send + 'static,
        Self: Handles<C, Idx>,
    {
        let id = id.clone();
        async move {
            Ok(Entity {
                aggregate_id: id,
                revision: Revision::zero(),
                state: Counter { value: 0 },
            })
        }
    }
}

fn main() {
    let service = TestService;
    let id: AggregateId = "counter:c1".parse().unwrap();
    // This should fail — TestService does not implement Handles<Unsupported>.
    // Note: the compiler diagnostic says "mismatched types" (Increment vs Unsupported)
    // rather than "missing Handles impl" — this is a known RPIT inference artifact
    // where the compiler infers C from the only Handles impl, then rejects the mismatch.
    let _ = service.execute(&id, Unsupported);
}
