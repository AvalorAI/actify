// The impl block is emitted alongside the errors, so its bodies compile too.
#![allow(unused_variables)]

// EXPECTED: every invalid argument is reported, not just the first one.
// Reporting one error per compile cycle forces users to fix a block of
// methods one recompile at a time.
use actify::actify;

#[derive(Clone, Debug)]
struct MyActor;

#[actify]
impl MyActor {
    fn first(&self, bad_ref: &str) {}

    fn second(&self, bad_ptr: *const u8) {}

    fn third(&self, bad_impl: impl Fn()) {}
}

fn main() {}
