#![warn(missing_debug_implementations, unreachable_pub)] // TODO enable 'missing_docs'
#![deny(unused_must_use)]
#![allow(clippy::unit_arg)]

//! An intuitive actor model with minimal required boilerplate code.
//!
//! Actify is an actor model built on [Tokio][tokio] that allows annotating any regular implementation block of your own type with the actify! macro.
//! By generating the boilerplate code for you, a few key benefits are provided:
//!
//! * Async access to an actor through multiple clonable handles
//! * Type-checked methods on your handles for all methods from the actors
//! * No need to define message structs or enums
//! * Atomic changes to the actor as requests are based on channels
//!
//! [tokio]: https://docs.rs/tokio/latest/tokio/
//!
//! # Main functionality of actify!
//!
//! Consider the following example:
//! ```
//! # use actor_model::{Handle, actify};
//! # #[derive(Clone)]
//! # struct Greeter {}
//!
//! #[actify]
//! impl Greeter {
//!     fn say_hi(&self, name: String) -> String {
//!         format!("hi {}", name)
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // An actify handle is created and initialized with the Greeter struct
//!     let handle = Handle::new_from(Greeter {});
//!
//!     // Although the say_hi method is implemented on the Greeter, it is automatically made available on its handle through the actify! macro
//!     let greeting = handle.say_hi("Alfred".to_string()).await.unwrap();
//!
//!     // The actual implementation of the method is executed asynchronously on the initialized Greeter and returned through the handle
//!     assert_eq!(greeting, "hi Alfred".to_string())
//! }
//! ```
//!
//! This roughly desugars to:
//! ```
//! # use actor_model::{Handle, actify, ActorError, Actor, FnType};
//! # #[derive(Clone)]
//! # struct Greeter {}
//! impl Greeter {
//!     fn say_hi(&self, name: String) -> String {
//!         format!("hi {}", name)
//!     }
//! }
//!
//! // Defines the custom function signatures that should be added to the handle
//! #[async_trait::async_trait]
//! pub trait GreeterHandle {
//!     async fn say_hi(&self, name: String) -> Result<String, ActorError>;
//! }
//!
//! // Implements the methods on the handle, and calls the generated method for the actor
//! #[async_trait::async_trait]
//! impl GreeterHandle for Handle<Greeter> {
//!     async fn say_hi(&self, name: String) -> Result<String, ActorError> {
//!         let res = self
//!             .send_job(FnType::Inner(Box::new(GreeterActor::_say_hi)), Box::new(name))
//!             .await?;
//!         Ok(*res.downcast().unwrap())
//!     }
//! }
//!
//! // Defines the wrappers that execute the original methods on the struct in the actor
//! trait GreeterActor {
//!     fn _say_hi(&mut self, args: Box<dyn std::any::Any + Send>) -> Result<Box<dyn std::any::Any + Send>, ActorError>;
//! }
//!
//! // Implements the methods on the actor for this specific type
//! impl GreeterActor for Actor<Greeter>
//! {
//!     fn _say_hi(&mut self, args: Box<dyn std::any::Any + Send>) -> Result<Box<dyn std::any::Any + Send>, ActorError> {
//!         let name: String = *args.downcast().unwrap();
//!
//!         // This call is the actual execution of the method from the user-defined impl block, on the struct held by the actor
//!         let result: String = self.inner.as_mut().unwrap().say_hi(name);  
//!         Ok(Box::new(result))
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle = Handle::new_from(Greeter {});
//!     let greeting = handle.say_hi("Alfred".to_string()).await.unwrap();
//!     assert_eq!(greeting, "hi Alfred".to_string())
//! }
//! ```
//!
//! As references cannot be send to the actor, they are forbidden. All types must be owned:
//! ```compile_fail
//! # struct MyActor {}
//! #[actor_model::actify]
//! impl MyActor {
//!     fn foo(&self, forbidden_reference: &usize) {
//!         println!("Hello foo: {}", forbidden_reference);
//!     }
//! }
//! ```

mod actors;
mod cache;
mod throttle;

// Reexport for easier reference
pub use actify_macros::actify;
pub use actors::any::{Actor, FnType, Handle};
pub use actors::map::MapHandle;
pub use actors::vec::VecHandle;
pub use actors::ActorError;
pub use async_trait::async_trait;
pub use cache::Cache;
pub use throttle::{Frequency, ThrottleBuilder, Throttled};

/// An example struct for the macro tests
#[allow(dead_code)]
#[derive(Clone)]
struct TestStruct<T> {
    inner_data: T,
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::collections::HashMap;

    use crate as actor_model; // used so that the expanded absolute path functions in this crate

    #[actify]
    impl<T> crate::TestStruct<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        #[cfg(not(feature = "test_feature"))]
        fn foo(&mut self, i: i32, _f: HashMap<String, f32>) -> f64 {
            (i + 1) as f64
        }
    }

    impl<T> TestStruct<T> {
        fn bar(&self, i: usize) -> f32 {
            (i + 1) as f32
        }
    }

    #[async_trait]
    trait ExampleTestStructHandle {
        async fn bar(&self, test: usize) -> Result<f32, ActorError>;
    }

    #[async_trait]
    impl<T> ExampleTestStructHandle for Handle<TestStruct<T>>
    where
        T: Clone + Send + Sync + 'static,
    {
        async fn bar(&self, test: usize) -> Result<f32, ActorError> {
            let res = self
                .send_job(FnType::Inner(Box::new(ExampleTestStructActor::_bar)), Box::new(test))
                .await?;
            Ok(*res.downcast().expect("Downcasting failed due to an error in the Actify macro"))
        }
    }

    trait ExampleTestStructActor {
        fn _bar(&mut self, args: Box<dyn std::any::Any + Send>) -> Result<Box<dyn std::any::Any + Send>, ActorError>;
    }

    #[allow(unused_parens)]
    impl<T> ExampleTestStructActor for Actor<TestStruct<T>>
    where
        T: Clone + Send + Sync + 'static,
    {
        fn _bar(&mut self, args: Box<dyn std::any::Any + Send>) -> Result<Box<dyn std::any::Any + Send>, ActorError> {
            let (test): (usize) = *args.downcast().expect("Downcasting failed due to an error in the Actify macro");

            let result: f32 = self
                .inner
                .as_mut()
                .ok_or(ActorError::NoValueSet(std::any::type_name::<TestStruct<T>>().to_string()))?
                .bar(test);

            self.broadcast();

            Ok(Box::new(result))
        }
    }

    #[tokio::test]
    async fn test_macro() {
        let actor_handle = Handle::new_from(TestStruct {
            inner_data: "Test".to_string(),
        });

        assert_eq!(actor_handle.bar(0).await.unwrap(), 1.);
        assert_eq!(actor_handle.foo(0, HashMap::new()).await.unwrap(), 1.)
    }
}
