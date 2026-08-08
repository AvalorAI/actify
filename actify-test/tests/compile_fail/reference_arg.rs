// The impl block is emitted alongside the errors, so its bodies compile too.
#![allow(unused_variables)]

// EXPECTED: This should fail with a clear compile_error! message.
// The macro correctly identifies reference arguments and rejects them.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify]
impl MyActor {
    fn foo(&self, bad_ref: &str) {}
}

fn main() {}
