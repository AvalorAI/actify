#![warn(missing_debug_implementations, unreachable_pub)]
#![deny(unused_must_use)]
#![allow(clippy::unit_arg)]
// TODO enable 'missing_docs'
// TODO decide wether initializing empty handles is possible or not (not backwords compatible)

//! An intuitive actor model for Rust with minimal boilerplate, no manual messages and typed arguments.
//!
//! Actify is an actor model built on [Tokio][tokio] that allows annotating any regular implementation block of your own type with the actify! macro.
//! By generating the boilerplate code for you, a few key benefits are provided:
//!
//! * Async actor model build on Tokio and channels
//! * Access to actors through clonable handles
//! * Types arguments on the methods from your actor, exposed through the handle
//! * No need to define message structs or enums!
//!
//! [tokio]: https://docs.rs/tokio/latest/tokio/
//!
//! # Main functionality of actify!
//!
//! Consider the following example, in which you want to turn your custom Greeter into an actor:
//! ```
//! # use actor_model::{Handle, actify};
//! # use std::fmt::Debug;
//! # #[derive(Clone, Debug)]
//! # struct Greeter {}
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
//!     // The say_hi method is made available on its handle through the actify! macro
//!     let greeting = handle.say_hi("Alfred".to_string()).await.unwrap();
//!
//!     // The method is executed on the initialized Greeter and returned through the handle
//!     assert_eq!(greeting, "hi Alfred".to_string())
//! }
//! ```
//!
//! This roughly desugars to:
//! ```
//! # use actor_model::{Handle, actify, ActorError, Actor, FnType};
//! # #[derive(Clone, Debug)]
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
//! ## Async functions in impl blocks
//! Async function are fully supported, and work as you would expect:
//! ```
//! # use actor_model::{Handle, actify};
//! # use std::fmt::Debug;
//! # #[derive(Clone, Debug)]
//! # struct AsyncGreeter {}
//! #[actify]
//! impl AsyncGreeter {
//!     async fn async_hi(&self, name: String) -> String {
//!         format!("hi {}", name)
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle = Handle::new_from(AsyncGreeter {});
//!     let greeting = handle.async_hi("Alfred".to_string()).await.unwrap();
//!     assert_eq!(greeting, "hi Alfred".to_string())
//! }
//! ```
//!
//! ## Generics in the actor type
//! Generics in the actor type are fully supported, as long as they implement Clone, Debug, Send, Sync and 'static:
//! ```
//! # use actor_model::{Handle, actify};
//! # use std::fmt::Debug;
//! # #[derive(Clone, Debug)]
//! struct GenericGreeter<T> {
//!     inner: T
//! }
//!
//! #[actify]
//! impl<T> GenericGreeter<T>
//! where
//!     T: Clone + Debug + Send + Sync + 'static,
//! {
//!     async fn generic_hi(&self, name: String) -> String {
//!         format!("hi {} from {:?}", name, self.inner)
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle = Handle::new_from(GenericGreeter { inner: usize::default() });
//!     let greeting = handle.generic_hi("Alfred".to_string()).await.unwrap();
//!     assert_eq!(greeting, "hi Alfred from 0".to_string())
//! }
//!```
//!
//! ## Generics in the method arguments
//! Unfortunately, passing generics by arguments is not yet supported. It is technically possible, and will be added in the near future.
//! ```compile_fail
//! # use actor_model::{Handle, actify};
//! # use std::fmt::Debug;
//! # #[derive(Clone, Debug)]
//! # struct Greeter { }
//! #[actify]
//! impl Greeter
//! {
//!     async fn generic_hi<F>(&self, name: String, f: F) -> String
//!     where
//!         F: Debug + Send + Sync,
//!     {
//!         format!("hi {} with {:?}", name, f)
//!     }
//! }
//!```
//!
//! ## Passing arguments by reference
//! As referenced arguments cannot be send to the actor, they are forbidden. All arguments must be owned:
//! ```compile_fail
//! # struct MyActor {}
//! #[actor_model::actify]
//! impl MyActor {
//!     fn foo(&self, forbidden_reference: &usize) {
//!         println!("Hello foo: {}", forbidden_reference);
//!     }
//! }
//! ```
//!
//! ## Standard actor methods
//! TODO: add documentation on the standard actor methods like get and set
//!
//! ## Atomicity
//! TODO: add documentation on how to guarantee atomocity by preventing updating actors with gets and sets
//!
//! ## Preventing cycles
//! TODO: add documentation on how to prevent cycles when actors use eachothers handles

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
#[derive(Clone, Debug)]
struct TestStruct<T> {
    inner_data: T,
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::{collections::HashMap, fmt::Debug};

    use crate as actor_model; // used so that the expanded absolute path functions in this crate

    #[actify]
    impl<T> crate::TestStruct<T>
    where
        T: Clone + Debug + Send + Sync + 'static,
    {
        #[cfg(not(feature = "test_feature"))]
        fn foo(&mut self, i: i32, _h: HashMap<String, T>) -> f64 {
            (i + 1) as f64
        }

        async fn baz(&mut self, i: i32) -> f64 {
            (i + 2) as f64
        }
    }

    impl<T> TestStruct<T>
    where
        T: Clone + Debug + Send + Sync + 'static,
    {
        // TODO generics in arguments are not supported yet
        fn bar<F>(&self, i: usize, _f: F) -> f32
        where
            F: Debug + Send + Sync,
        {
            (i + 1) as f32
        }
    }

    #[async_trait]
    trait ExampleTestStructHandle<F> {
        async fn bar(&self, test: usize, f: F) -> Result<f32, ActorError>;
    }

    #[async_trait]
    impl<T, F> ExampleTestStructHandle<F> for Handle<TestStruct<T>>
    where
        T: Clone + Debug + Send + Sync + 'static,
        F: Debug + Send + Sync + 'static,
    {
        async fn bar(&self, test: usize, t: F) -> Result<f32, ActorError> {
            let res = self
                .send_job(
                    FnType::Inner(Box::new(ExampleTestStructActor::<F>::_bar)),
                    Box::new((test, t)),
                )
                .await?;
            Ok(*res
                .downcast()
                .expect("Downcasting failed due to an error in the Actify macro"))
        }
    }

    trait ExampleTestStructActor<F> {
        fn _bar(
            &mut self,
            args: Box<dyn std::any::Any + Send>,
        ) -> Result<Box<dyn std::any::Any + Send>, ActorError>;
    }

    #[allow(unused_parens)]
    impl<T, F> ExampleTestStructActor<F> for Actor<TestStruct<T>>
    where
        T: Clone + Debug + Send + Sync + 'static,
        F: Debug + Send + Sync + 'static,
    {
        fn _bar(
            &mut self,
            args: Box<dyn std::any::Any + Send>,
        ) -> Result<Box<dyn std::any::Any + Send>, ActorError> {
            let (test, f): (usize, F) = *args
                .downcast()
                .expect("Downcasting failed due to an error in the Actify macro");

            let result: f32 = self
                .inner
                .as_mut()
                .ok_or(ActorError::NoValueSet(
                    std::any::type_name::<TestStruct<T>>().to_string(),
                ))?
                .bar(test, f);

            self.broadcast();

            Ok(Box::new(result))
        }
    }

    #[tokio::test]
    async fn test_macro() {
        let actor_handle = Handle::new_from(TestStruct {
            inner_data: "Test".to_string(),
        });

        assert_eq!(actor_handle.bar(0, String::default()).await.unwrap(), 1.);
        assert_eq!(actor_handle.foo(0, HashMap::new()).await.unwrap(), 1.);
        assert_eq!(actor_handle.baz(0).await.unwrap(), 2.);
    }
}
