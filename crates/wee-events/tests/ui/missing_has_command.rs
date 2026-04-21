use wee_events::{Command, HasCommand};

struct Service;
#[derive(serde::Serialize)]
struct Unknown;
impl Command for Unknown {
    const NAME: &'static str = "x";
}

fn require<T: HasCommand<Unknown>>() {}

fn main() {
    require::<Service>();
}
