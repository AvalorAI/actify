// Unsupported argument types (e.g. slices) should be rejected
// with a clear error message suggesting concrete owned types.
use actify::actify;

#[derive(Clone, Debug)]
struct SliceActor;

#[actify]
impl SliceActor {
    fn with_slice(&self, data: [u8]) -> u8 {
        data[0]
    }
}

fn main() {}
