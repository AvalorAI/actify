#![warn(missing_debug_implementations, unreachable_pub)]
#![deny(unused_must_use)]
//! An intuitive actor model for Rust with minimal boilerplate, no manual messages and typed arguments.
//!
//! **Note that this crate is under construction. Although used in production, work is done on making an intuitive API, documententation and remaining features. Pre 1.0 the API may break at any time!**
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
//! # use actify::{Handle, actify};
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
//!     let handle = Handle::new(Greeter {});
//!
//!     // The say_hi method is made available on its handle through the actify! macro
//!     let greeting = handle.say_hi("Alfred".to_string()).await;
//!
//!     // The method is executed on the initialized Greeter and returned through the handle
//!     assert_eq!(greeting, "hi Alfred".to_string())
//! }
//! ```
//!
//! This roughly desugars to:
//! ```
//! # use actify::{Handle, actify, Actor};
//! # #[derive(Clone, Debug)]
//! # struct Greeter {}
//! impl Greeter {
//!     fn say_hi(&self, name: String) -> String {
//!         format!("hi {}", name)
//!     }
//! }
//!
//! // Defines the custom function signatures that should be added to the handle
//! pub trait GreeterHandle {
//!     async fn say_hi(&self, name: String) -> String;
//! }
//!
//! // Implements the methods on the handle, and calls the generated method for the actor
//! impl GreeterHandle for Handle<Greeter> {
//!     async fn say_hi(&self, name: String) -> String {
//!         let res = self
//!             .send_job(
//!                         Box::new(|s: &mut Actor<Greeter>, args: Box<dyn std::any::Any + Send>| {
//!                     Box::pin(async move { GreeterActor::_say_hi(s, args).await })
//!                 }),
//!                 Box::new(name),
//!             )
//!             .await;
//!
//!         *res.downcast().unwrap()
//!     }
//! }
//!
//! // Defines the wrappers that execute the original methods on the struct in the actor
//! trait GreeterActor {
//!     async fn _say_hi(&mut self, args: Box<dyn std::any::Any + Send>) -> Box<dyn std::any::Any + Send>;
//! }
//!
//! // Implements the methods on the actor for this specific type
//! impl GreeterActor for Actor<Greeter>
//! {
//!     async fn _say_hi(&mut self, args: Box<dyn std::any::Any + Send>) -> Box<dyn std::any::Any + Send> {
//!         let name: String = *args.downcast().unwrap();
//!
//!         // This call is the actual execution of the method from the user-defined impl block, on the struct held by the actor
//!         let result: String = self.inner.say_hi(name);
//!         Box::new(result)
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle = Handle::new(Greeter {});
//!     let greeting = handle.say_hi("Alfred".to_string()).await;
//!     assert_eq!(greeting, "hi Alfred".to_string())
//! }
//! ```
//!
//! ## Async functions in impl blocks
//! Async function are fully supported, and work as you would expect:
//! ```
//! # use actify::{Handle, actify};
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
//!     let handle = Handle::new(AsyncGreeter {});
//!     let greeting = handle.async_hi("Alfred".to_string()).await;
//!     assert_eq!(greeting, "hi Alfred".to_string())
//! }
//! ```
//!
//! ## Generics in the actor type
//! Generics in the actor type are fully supported, as long as they implement Clone, Debug, Send, Sync and 'static:
//! ```
//! # use actify::{Handle, actify};
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
//!     let handle = Handle::new(GenericGreeter { inner: usize::default() });
//!     let greeting = handle.generic_hi("Alfred".to_string()).await;
//!     assert_eq!(greeting, "hi Alfred from 0".to_string())
//! }
//!```
//!
//! ## Generics in the method arguments
//! Unfortunately, passing generics by arguments is not yet supported. It is technically possible, and will be added in the near future.
//! ```compile_fail
//! # use actify::{Handle, actify};
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
//! TODO: show temporary workaround with PhantomData in the actor struct
//!
//! ## Passing arguments by reference
//! As referenced arguments cannot be send to the actor, they are forbidden. All arguments must be owned:
//! ```compile_fail
//! # struct MyActor {}
//! #[actify::actify]
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
mod extensions;
mod throttle;

// Reexport for easier reference
pub use actify_macros::{actify, broadcast, skip_broadcast};
pub use actors::{Actor, Handle, ReadHandle};
pub use cache::{Cache, CacheRecvError, CacheRecvNewestError};
pub use extensions::{
    map::HashMapHandle, option::OptionHandle, set::HashSetHandle, vec::VecHandle,
};
pub use throttle::{Frequency, Throttle, Throttled};

#[cfg(feature = "profiler")]
pub use actors::{get_broadcast_counts, get_sorted_broadcast_counts};
