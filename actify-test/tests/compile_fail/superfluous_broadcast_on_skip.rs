// EXPECTED: broadcast attributes are meaningless on a skipped method, since it
// is never generated for.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify]
impl MyActor {
    #[actify::skip]
    #[actify::broadcast]
    fn one(&self) {}

    #[actify::skip]
    #[actify::skip_broadcast]
    fn two(&mut self) {}
}

fn main() {}
