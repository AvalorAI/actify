// EXPECTED: #[actify(broadcast)] is unnecessary because methods already broadcast by default.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify(broadcast)]
impl MyActor {
    fn some_method(&self) {}
}

fn main() {}
