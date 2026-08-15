#![warn(missing_docs, missing_debug_implementations, unreachable_pub)]
#![deny(unused_must_use)]
//! An intuitive actor model for Rust with minimal boilerplate, no manual messages and typed arguments.
//!
//! Actify is a pre-1.0 crate used in production. The API may still change between minor versions.
//!
//! Sharing (mutable) state across async tasks in Rust usually means juggling mutexes and channels,
//! and a lot of boilerplate like hand-writen message enums. Actify gives you a typed, async
//! actor model built on [Tokio][tokio] for any struct. Just add `#[actify]` to an `impl` block and call your methods
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
//! # use actify::{Handle, actify};
//! # use actify::__private::Actor;
//! # #[derive(Clone, Debug)]
//! # struct Greeter {}
//! impl Greeter {
//!     fn say_hi(&self, name: String) -> String {
//!         format!("hi {}", name)
//!     }
//! }
//!
//! // Defines the method signatures exposed on the handle. The future is
//! // promised to be Send, so a caller generic over this trait can spawn the
//! // call; `async fn` in a trait would not say that.
//! pub trait GreeterHandle {
//!     fn say_hi(&self, name: String) -> impl std::future::Future<Output = String> + Send;
//! }
//!
//! // Implements the methods: boxes args, sends a job to the actor,
//! // which downcasts them and calls the original method on the inner type
//! #[allow(unused_parens)]
//! impl GreeterHandle for Handle<Greeter> {
//!     async fn say_hi(&self, name: String) -> String {
//!         let res = self
//!             .__send_job(
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
//! - [`Handle::get`]: returns the actor's view, which for a plain `Clone` actor is a
//!   clone of the value itself (does not broadcast)
//! - [`Handle::set`]: overwrites the actor value (broadcasts the change)
//! - [`Handle::set_if_changed`]: only broadcasts when the new value differs (requires `PartialEq`)
//! - [`Handle::subscribe`]: returns a [`tokio::sync::broadcast::Receiver`] for change notifications
//! - [`Handle::with`]: runs a read-only closure on `&T`, the actor type rather than
//!   its view (does not broadcast)
//! - [`Handle::with_mut`]: runs a mutable closure on `&mut T` (broadcasts the change)
//! - [`Handle::wait_until`]: waits until the broadcast value satisfies a predicate
//!
//! # Leaving methods off the handle
//!
//! Every method in an `#[actify]` block gets a handle method. For an inherent
//! impl, the plainest way to keep one off the handle is a second `impl` block,
//! which needs nothing from actify.
//!
//! A trait impl cannot be split that way: Rust requires every method of a trait
//! in one impl block. Mark the method
//! [`#[actify::skip]`](macro@crate::skip) instead:
//!
//! ```
//! # use actify::actify;
//! # #[derive(Clone, Debug)]
//! # struct Ledger { entries: Vec<String> }
//! trait Merge {
//!     fn count(&self) -> usize;
//!
//!     fn merge(&mut self, other: &Ledger);
//! }
//!
//! #[actify]
//! impl Merge for Ledger {
//!     fn count(&self) -> usize {
//!         self.entries.len()
//!     }
//!
//!     // Takes a reference, which an actor call cannot
//!     #[actify::skip]
//!     fn merge(&mut self, other: &Ledger) {
//!         self.entries.extend(other.entries.iter().cloned());
//!     }
//! }
//! ```
//!
//! The method stays on the type, unchanged, and is not checked, so it may take
//! or return references.
//!
//! # Broadcasting
//!
//! A method taking `&mut self` broadcasts the updated value to all subscribers
//! after it returns. This keeps [`Cache`]s and [`Throttle`]s synchronized with
//! the actor. A method taking `&self` does not broadcast.
//!
//! ```
//! # use actify::{Handle, actify};
//! # #[derive(Clone, Debug)]
//! # struct Counter { value: i32 }
//! #[actify]
//! impl Counter {
//!     fn increment(&mut self) -> i32 {
//!         self.value += 1;
//!         self.value
//!     }
//!
//!     fn value(&self) -> i32 {
//!         self.value
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle = Handle::new(Counter { value: 0 });
//!     let mut rx = handle.subscribe();
//!
//!     handle.value().await;
//!     assert!(rx.try_recv().is_err());
//!
//!     handle.increment().await;
//!     assert_eq!(rx.try_recv().unwrap().value, 1);
//! }
//! ```
//!
//! Three attributes override the receiver:
//!
//! - `#[actify::broadcast]`: broadcast from a method taking `&self`
//! - `#[actify::skip_broadcast]`: do not broadcast from a method taking `&mut self`
//! - `#[actify(skip_broadcast)]`: no method in the impl block broadcasts unless it
//!   carries `#[actify::broadcast]`
//!
//! `#[actify::broadcast]` applies where a `&self` method changes what subscribers
//! observe, which requires interior mutability:
//!
//! ```
//! # use actify::{Handle, actify, ToView};
//! # use std::sync::Mutex;
//! # #[derive(Debug)]
//! # struct Counter { value: Mutex<i32> }
//! # impl ToView<i32> for Counter {
//! #     fn to_view(&self) -> i32 { *self.value.lock().unwrap() }
//! # }
//! #[actify]
//! impl Counter {
//!     #[actify::broadcast]
//!     fn increment(&self) -> i32 {
//!         let mut value = self.value.lock().unwrap();
//!         *value += 1;
//!         *value
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle: Handle<Counter, i32> = Handle::new(Counter { value: Mutex::new(0) });
//!     let mut rx = handle.subscribe();
//!
//!     assert_eq!(handle.increment().await, 1);
//!     assert_eq!(rx.try_recv().unwrap(), 1);
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
//! #[actify(name = "CounterGetters")]
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
//! to its broadcast stream. Create one with [`Handle::cache`] (initialized with the
//! current value), [`Handle::cache_from`] (custom initial value),
//! or [`Handle::cache_from_default`] (starts from `T::default()`).
//!
//! [`Cache::wait_until`] waits until the cached value satisfies a predicate,
//! reading through the cache so it holds the value that matched.
//!
//! See [`CacheRecvError`] for the possible error conditions.
//!
//! # ReadHandle
//!
//! A [`ReadHandle`] is a read-only view of an actor. It supports [`get`](ReadHandle::get),
//! [`subscribe`](ReadHandle::subscribe), [`wait_until`](ReadHandle::wait_until), cache
//! creation and throttle spawning, but cannot mutate the actor. Obtain one via
//! [`Handle::read_handle`].
//!
//! # Throttle
//!
//! A [`Throttle`] rate-limits broadcasted updates before forwarding them to a callback.
//! Configure the rate with [`Frequency`]:
//!
//! - [`Frequency::OnEvent`]: sends every value as it arrives
//! - [`Frequency::Interval`]: sends the current value every interval
//! - [`Frequency::OnEventWhen`]: sends at most once per interval, and only when a
//!   value arrived since the last send
//!
//! The callback can take a type other than the view: implement [`ToView<F>`] on
//! the view type for the payload `F` you want, and the callback signature selects
//! it. One actor can feed several throttles that way, each with its own payload.
//!
//! A throttle is spawned from a [`Handle`], a [`ReadHandle`] or a [`Cache`].
//!
//! [`Handle::spawn_async_throttle`], [`ReadHandle::spawn_async_throttle`] and
//! [`Cache::spawn_async_throttle`] take an async callback. Each call is awaited before the throttle looks for the next
//! value, so a callback slower than the [`Frequency`] delays the following send
//! rather than running alongside it.
//!
//! The callback borrows the client and returns a [`BoxFuture`], so it is shaped
//! `|client, value| Box::pin(..)`. An `async fn` taking `&self` wraps directly:
//! `|db, value| Box::pin(db.write(value))`. See
//! [`Handle::spawn_async_throttle`] for the longer forms.
//!
//! One call runs at a time either way, and nothing is received while it does.
//! See [Slow calls](Throttle#slow-calls) for what that means for the updates
//! arriving meanwhile.
//!
//! Spawning a throttle returns a [`Throttle`] handle. Dropping it leaves the
//! throttle running, so a throttle attached to an actor can be spawned and
//! forgotten: it stops when the actor does. [`Throttle::spawn_interval`] has no
//! actor attached and runs until [`Throttle::abort`] or the runtime shuts down,
//! which is why it is the one spawn that must not have its handle discarded.
//!
//! # Extension traits
//!
//! Actify ships with extension traits that add convenience methods to handles
//! wrapping common standard library types:
//!
//! - [`OptionHandle`] for `Handle<Option<T>>`
//! - [`VecHandle`] for `Handle<Vec<T>>`
//! - [`HashMapHandle`] for `Handle<HashMap<K, V>>`
//! - [`HashSetHandle`] for `Handle<HashSet<K>>`
//! - [`StringHandle`] for `Handle<String>`
//! - [`VecDequeHandle`] for `Handle<VecDeque<T>>`
//!
//! # Views and non-Clone types
//!
//! A handle exposes a view of its actor: the type `V` that [`Handle::get`]
//! returns and that the actor broadcasts. By default `V = T`, so
//! [`Handle::new`] requires `T: Clone` and the view is a clone of the value.
//!
//! For a non-Clone type, or to expose a summary instead of the whole value,
//! implement [`ToView<V>`] for a Clone-able `V` and name it explicitly:
//! `Handle::<MyType, Summary>::new(val)`. Reads then return the summary, and
//! [`Handle::with`] reads the actor type itself. Your `#[actify]` methods work
//! normally either way.
//!
//! # Execution model
//!
//! Each actor is one Tokio task. It runs one job at a time and takes the next
//! only after the current one returns, so calls never interleave and
//! `&mut self` methods need no lock. A slow method delays every other call to
//! that actor.
//!
//! Jobs queue in a bounded channel, so callers wait when it is full.
//!
//! Two actors that call each other never return: each waits for a reply the
//! other can only produce once it is free. The same holds for a method calling
//! its own handle.
//!
//! ```no_run
//! # use actify::{Handle, actify};
//! #[derive(Clone, Debug)]
//! struct Parser {
//!     store: Option<Handle<Store>>,
//! }
//!
//! #[derive(Clone, Debug)]
//! struct Store {
//!     parser: Option<Handle<Parser>>,
//! }
//!
//! #[actify]
//! impl Parser {
//!     async fn parse(&self) {
//!         self.store.as_ref().unwrap().save().await;
//!     }
//!
//!     async fn is_ready(&self) -> bool {
//!         true
//!     }
//! }
//!
//! #[actify]
//! impl Store {
//!     async fn save(&self) {
//!         // Parser is still inside parse, so this call is never served
//!         self.parser.as_ref().unwrap().is_ready().await;
//!     }
//! }
//! ```
//!
//! # Actor lifetime and panics
//!
//! An actor runs until every [`Handle`] to it is dropped. A [`ReadHandle`]
//! holds a handle internally and keeps the actor alive. A [`Cache`] does not,
//! since it only receives broadcasts.
//!
//! A panicking method stops the actor permanently. There is no restart, and
//! every later call on any handle to it panics, reporting that the actor
//! panicked. Rust prints the original panic with its message and backtrace.
//!
//! Calls after the actor's runtime has shut down panic too, reporting that the
//! actor is no longer running.
//!
//! # Feature flags
//!
//! - `profiler`: counts broadcasts per method, readable through
//!   `get_broadcast_counts` and `get_sorted_broadcast_counts`. Counters are
//!   process-wide and never reset.

/// The README examples, compiled and run as part of the test suite.
///
/// Only present under `cfg(doctest)`, so it costs nothing when building docs.
#[cfg(doctest)]
mod readme {
    #![doc = include_str!("../../README.md")]
}

// Generated code refers to ::actify, which has to resolve inside this crate too.
extern crate self as actify;

mod actor;
mod cache;
mod extensions;
mod handles;
mod throttle;

// Reexport for easier reference
pub use actify_macros::{actify, broadcast, skip, skip_broadcast};
pub use cache::{Cache, CacheRecvError};
pub use extensions::{
    map::HashMapHandle, option::OptionHandle, set::HashSetHandle, string::StringHandle,
    vec::VecHandle, vecdeque::VecDequeHandle,
};
pub use handles::{Handle, ReadHandle, ToView};
pub use throttle::{BoxFuture, Frequency, Throttle};

/// The crate's own items that the [`actify`](macro@crate::actify) macro needs in
/// generated code. Standard library types are named by absolute path instead.
///
/// Not part of the public API. Anything here may change in a patch release, and
/// nothing outside generated code should name it.
#[doc(hidden)]
pub mod __private {
    pub use crate::actor::Actor;
}

#[cfg(feature = "profiler")]
pub use actor::{get_broadcast_counts, get_sorted_broadcast_counts};
