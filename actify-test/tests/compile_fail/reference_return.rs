// EXPECTED: a clear message that return types must be owned.
// Results travel back from the actor task as Box<dyn Any + Send>, which
// requires 'static, so a borrow of the actor state cannot escape.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor {
    name: String,
}

#[actify]
impl MyActor {
    fn name(&self) -> &str {
        &self.name
    }
}

fn main() {}
