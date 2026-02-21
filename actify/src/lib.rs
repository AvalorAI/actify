#![warn(missing_debug_implementations, unreachable_pub)]
#![deny(unused_must_use)]
//! An intuitive actor model for Rust with minimal boilerplate, no manual messages and typed arguments.
//!
//! Actify is a pre-1.0 crate used in production. The API may still change between minor versions.
//!
//! Sharing (mutable) state across async tasks in Rust usually means juggling mutexes and channels,
//! and a lot of boilerplate like hand-writen message enums. Actify gives you a typed, async
//! actor model built on [Tokio][tokio] for any struct — just add `#[actify]` to an `impl` block and call your methods
//! through a clonable [`Handle`].
//!
//! By generating the boilerplate code for you, a few key benefits are provided:
//!
//! * Async actor model built on Tokio and channels
//! * Access to actors through clonable [`Handle`]s
//! * Typed arguments on the methods from your actor, exposed through the handle
//! * No need to define message structs or enums!
//! * Automatic [broadcasting] of state changes to subscribers
//! * Local synchronization through [`Cache`]
//! * Rate-limited updates through [`Throttle`]
//! * Built-in [extension traits] for common standard library types
//!
//! [tokio]: https://docs.rs/tokio/latest/tokio/
//! [broadcasting]: #broadcasting
//! [extension traits]: #extension-traits
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
//! // Defines the method signatures exposed on the handle
//! pub trait GreeterHandle {
//!     async fn say_hi(&self, name: String) -> String;
//! }
//!
//! // Implements the methods: boxes args, sends a job to the actor,
//! // which downcasts them and calls the original method on the inner type
//! #[allow(unused_parens)]
//! impl GreeterHandle for Handle<Greeter> {
//!     async fn say_hi(&self, name: String) -> String {
//!         let res = self
//!             .send_job(
//!                 Box::new(
//!                     |s: &mut Actor<Greeter>, args: Box<dyn std::any::Any + Send>|
//!                     Box::pin(async move {
//!                         let name: String = *args.downcast().unwrap();
//!                         let result: String = Greeter::say_hi(&s.inner, name);
//!                         s.broadcast("Greeter::say_hi");
//!                         Box::new(result) as Box<dyn std::any::Any + Send>
//!                     })),
//!                 Box::new(name),
//!             )
//!             .await;
//!
//!         *res.downcast().unwrap()
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
//! Generic method parameters are supported when they have appropriate trait bounds.
//! The type parameters must be `Send + Sync + 'static`:
//! ```
//! # use actify::{Handle, actify};
//! # use std::fmt::Debug;
//! # #[derive(Clone, Debug)]
//! # struct Greeter { }
//! #[actify]
//! impl Greeter
//! {
//!     fn apply<F>(&self, value: usize, f: F) -> usize
//!     where
//!         F: Fn(usize) -> usize + Send + Sync + 'static,
//!     {
//!         f(value)
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle = Handle::new(Greeter {});
//!     let result = handle.apply(5, |x| x * 2).await;
//!     assert_eq!(result, 10);
//! }
//!```
//!
//! ## Passing arguments by reference
//! As referenced arguments cannot be sent to the actor, they are forbidden. All arguments must be owned:
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
//! # Standard methods
//!
//! Every [`Handle`] provides a set of built-in methods that work without the macro:
//!
//! - [`Handle::get`] — returns a clone of the current actor value (does not broadcast)
//! - [`Handle::set`] — overwrites the actor value (broadcasts the change)
//! - [`Handle::set_if_changed`] — only broadcasts when the new value differs (requires `PartialEq`)
//! - [`Handle::subscribe`] — returns a [`tokio::sync::broadcast::Receiver`] for change notifications
//! - [`Handle::with`] — runs a read-only closure on `&T` (does not broadcast)
//! - [`Handle::with_mut`] — runs a mutable closure on `&mut T` (broadcasts the change)
//!
//! # Broadcasting
//!
//! By default, any method on an actified type automatically broadcasts the
//! updated value to all subscribers after execution. This allows [`Cache`]s and
//! [`Throttle`]s to stay synchronized with the actor.
//!
//! You can control broadcasting with these attributes:
//!
//! - `#[actify::skip_broadcast]` — skip broadcasting for a single `&mut self` method
//! - `#[actify(skip_broadcast)]` — skip broadcasting for all methods in the impl block
//! - `#[actify::broadcast]` — force broadcasting for a method in a `skip_broadcast` block
//!
//! ```
//! # use actify::{Handle, actify, skip_broadcast, broadcast};
//! # use std::fmt::Debug;
//! # #[derive(Clone, Debug)]
//! # struct Counter { value: i32 }
//! #[actify(skip_broadcast)]
//! impl Counter {
//!     /// Does not broadcast (block default).
//!     fn increment(&mut self) -> i32 {
//!         self.value += 1;
//!         self.value
//!     }
//!
//!     /// Overrides the block default to broadcast.
//!     #[actify::broadcast]
//!     fn reset(&mut self) -> i32 {
//!         self.value = 0;
//!         self.value
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle = Handle::new(Counter { value: 0 });
//!     let mut rx = handle.subscribe();
//!
//!     handle.increment().await; // No broadcast
//!     assert!(rx.try_recv().is_err());
//!
//!     handle.reset().await; // Broadcasts
//!     assert!(rx.try_recv().is_ok());
//! }
//! ```
//!
//! ## Multiple impl blocks
//!
//! Each `#[actify]` block generates a trait named `{Type}Handle`. To use multiple
//! impl blocks on the same type, provide a custom trait name with `name = "..."` to
//! avoid collisions:
//!
//! ```
//! # use actify::{Handle, actify};
//! # #[derive(Clone, Debug)]
//! struct Counter { value: i32 }
//!
//! #[actify]
//! impl Counter {
//!     fn increment(&mut self) { self.value += 1; }
//! }
//!
//! #[actify(name = "CounterGetters", skip_broadcast)]
//! impl Counter {
//!     fn value(&self) -> i32 { self.value }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle = Handle::new(Counter { value: 0 });
//!     handle.increment().await;
//!     assert_eq!(handle.value().await, 1);
//! }
//! ```
//!
//! The first block generates `CounterHandle`, the second generates `CounterGetters`.
//! Both traits are automatically implemented for `Handle<Counter>`.
//!
//! # Cache
//!
//! A [`Cache`] provides local, synchronous access to the actor's value by subscribing
//! to its broadcast stream. Create one with [`Handle::create_cache`] (initialized with the
//! current value), [`Handle::create_cache_from`] (custom initial value),
//! or [`Handle::create_cache_from_default`] (starts from `T::default()`).
//!
//! See [`CacheRecvError`] and [`CacheRecvNewestError`] for possible error conditions.
//!
//! # ReadHandle
//!
//! A [`ReadHandle`] is a read-only view of an actor. It supports [`get`](ReadHandle::get),
//! [`subscribe`](ReadHandle::subscribe), and cache creation, but cannot mutate the actor.
//! Obtain one via [`Handle::get_read_handle`].
//!
//! # Throttle
//!
//! A [`Throttle`] rate-limits broadcasted updates before forwarding them to a callback.
//! Configure the rate with [`Frequency`]:
//!
//! - [`Frequency::OnEvent`] — fires on every broadcast
//! - [`Frequency::Interval`] — fires at a fixed interval
//! - [`Frequency::OnEventWhen`] — fires for an event only after an interval has passed
//!
//! Use the [`Throttled`] trait to parse the actor's type into a different output type
//! for the throttle callback.
//!
//! # Extension traits
//!
//! Actify ships with extension traits that add convenience methods to handles
//! wrapping common standard library types:
//!
//! - [`OptionHandle`] — `is_some`, `is_none` for `Handle<Option<T>>`
//! - [`VecHandle`] — `push`, `is_empty`, `drain` for `Handle<Vec<T>>`
//! - [`HashMapHandle`] — `get_key`, `insert`, `is_empty` for `Handle<HashMap<K, V>>`
//! - [`HashSetHandle`] — `insert`, `is_empty` for `Handle<HashSet<K>>`
//!
//! # Non-Clone types
//!
//! By default, [`Handle::new`] requires `T: Clone` so it can broadcast `T`.
//! For non-Clone types, implement [`BroadcastAs<V>`] for a Clone-able summary
//! type `V` and specify it explicitly: `Handle::<MyType, Summary>::new(val)`.
//! Your `#[actify]` methods work normally either way.

mod actor;
mod cache;
mod extensions;
mod handles;
mod throttle;

// Reexport for easier reference
pub use actify_macros::{actify, broadcast, skip_broadcast};
pub use cache::{Cache, CacheRecvError, CacheRecvNewestError};
pub use extensions::{
    map::HashMapHandle, option::OptionHandle, set::HashSetHandle, vec::VecHandle,
};
pub use handles::{BroadcastAs, Handle, ReadHandle};
pub use throttle::{Frequency, Throttle, Throttled};

#[doc(hidden)]
pub use actor::Actor;

#[cfg(feature = "profiler")]
pub use actor::{get_broadcast_counts, get_sorted_broadcast_counts};
