mod actors;
mod cache;
mod throttle;

// Reexport for easier reference
pub use actify_macros::actify;
pub use actors::any::Handle;
pub use actors::map::MapHandle;
pub use actors::vec::VecHandle;
pub use actors::ActorError;
pub use cache::Cache;
pub use throttle::{Frequency, ThrottleBuilder, Throttled};

#[cfg(test)]
mod tests {
    use actify_macros::actify;

    struct MyActor {}

    #[actify]
    impl MyActor {
        fn foo(&mut self, i: i32, f: f32) {
            println!("Hello foo: {}, {}", i, f);
        }

        fn bar(&self, test: usize) {
            println!("Hello bar: {}", test);
        }
    }

    #[test]
    fn test_macro() {
        let mut actor = MyActor {};
        actor.handle_foo(1, 4.5);
        actor.handle_bar(2);
    }
}
