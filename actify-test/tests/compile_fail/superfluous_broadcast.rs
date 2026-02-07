// EXPECTED: #[broadcast] on a method without #[actify(skip_broadcast)] on impl is superfluous.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify]
impl MyActor {
    #[actify::broadcast]
    fn already_broadcasts(&self) {}
}

fn main() {}
