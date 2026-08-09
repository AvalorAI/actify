// EXPECTED: #[skip_broadcast] on a &self method is superfluous, it never broadcasts.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify]
impl MyActor {
    #[actify::skip_broadcast]
    fn never_broadcasts(&self) {}
}

fn main() {}
