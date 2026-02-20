// EXPECTED: invalid `name` value: must be a valid Rust identifier
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify(name = "123invalid")]
impl MyActor {
    fn some_method(&self) {}
}

fn main() {}
