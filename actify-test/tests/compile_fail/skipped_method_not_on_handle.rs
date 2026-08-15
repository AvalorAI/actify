// EXPECTED: a skipped method is absent from the handle trait, so calling it
// through a handle does not compile.
use actify::{Handle, actify};

#[derive(Clone, Debug)]
struct MyActor {
    value: i32,
}

#[actify]
impl MyActor {
    fn exposed(&self) -> i32 {
        self.value
    }

    #[actify::skip]
    fn hidden(&self) -> i32 {
        self.value
    }
}

#[tokio::main]
async fn main() {
    let handle = Handle::new(MyActor { value: 1 });

    let _ = handle.exposed().await;
    let _ = handle.hidden().await;
}
