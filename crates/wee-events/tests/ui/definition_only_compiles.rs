use wee_events::Command;

#[derive(Default)]
pub struct S;
#[derive(serde::Serialize)]
struct C;
impl Command for C {
    const NAME: &'static str = "x";
}

wee_events::service! { pub Svc("s") for S [C] }

fn main() {}
