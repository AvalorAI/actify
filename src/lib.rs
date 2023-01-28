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

    impl MyActor {
        #[actify]
        fn some_function(&mut self, i: i32, f: f32) {
            println!("Hello {}, World ! {}", i, f);
        }
    }

    #[test]
    fn test_macro() {
        let mut actor = MyActor {};
        actor.handle_some_function(1, 4.5)
    }
}
