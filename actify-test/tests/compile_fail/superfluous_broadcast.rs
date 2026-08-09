// EXPECTED: #[broadcast] on a &mut self method is superfluous, it broadcasts by default.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify]
impl MyActor {
    #[actify::broadcast]
    fn already_broadcasts(&mut self) {}
}

fn main() {}
