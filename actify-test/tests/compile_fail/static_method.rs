// Static methods (without &self or &mut self) cannot be actified
// because the actor model requires a receiver to forward calls through.
use actify::actify;

#[derive(Clone, Debug)]
struct StaticActor;

#[actify]
impl StaticActor {
    fn no_receiver(x: i32) -> i32 {
        x + 1
    }
}

fn main() {}
