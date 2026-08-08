// EXPECTED: a clear message that unsafe methods cannot be actified.
// The generated handle calls the method from safe code, so without a
// dedicated check the failure surfaces inside generated tokens.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify]
impl MyActor {
    unsafe fn danger(&self) -> i32 {
        42
    }
}

fn main() {}
