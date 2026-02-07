// EXPECTED: #[skip_broadcast] on a method inside #[actify(skip_broadcast)] impl is superfluous.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify(skip_broadcast)]
impl MyActor {
    #[actify::skip_broadcast]
    fn already_skipped(&self) {}
}

fn main() {}
