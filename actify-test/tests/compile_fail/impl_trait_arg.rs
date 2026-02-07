// impl Trait cannot be boxed as dyn Any for actor channel transport.
// The macro should reject it and suggest using a named generic type parameter instead.
use actify::actify;

#[derive(Clone, Debug)]
struct ImplTraitActor;

#[actify]
impl ImplTraitActor {
    fn with_impl_trait(&self, f: impl Fn(usize) -> usize) -> usize {
        f(42)
    }
}

fn main() {}
