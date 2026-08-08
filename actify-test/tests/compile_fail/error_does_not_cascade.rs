// EXPECTED: only the macro's own diagnostic is reported.
// When the macro discards the impl block on error, the type loses every
// method in it, so each call site produces an extra "no method named ..."
// error that buries the real cause.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify]
impl MyActor {
    fn broken(&self, _bad_ref: &str) {}

    fn healthy(&self) -> i32 {
        42
    }
}

fn main() {
    let actor = MyActor;
    actor.healthy();
    actor.broken("x");
}
