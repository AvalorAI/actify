// Raw pointers are not Send and cannot be used as actor method arguments.
// The macro should reject them with a clear compile_error!.
use actify::actify;

#[derive(Clone, Debug)]
struct PtrActor;

#[actify]
impl PtrActor {
    fn with_ptr(&self, ptr: *const u8) -> u8 {
        unsafe { *ptr }
    }
}

fn main() {}
