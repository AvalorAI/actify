# Actify

Actify is a pre-1.0 crate used in production. The API may still change between minor versions.

Sharing (mutable) state across async tasks in Rust usually means juggling mutexes and channels, and a lot of boilerplate like hand-writen message enums. Actify gives you a typed, async [actor model](https://en.wikipedia.org/wiki/Actor_model) built on [Tokio](https://tokio.rs) for any struct. Just add `#[actify]` to an `impl` block and call your methods through a clonable [`Handle`].

[![Crates.io][crates-badge]][crates-url]
[![License][mit-badge]][mit-url]
[![Docs][docs-badge]][docs-url]

[crates-url]: https://crates.io/crates/actify
[crates-badge]: https://img.shields.io/crates/v/actify.svg
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: https://github.com/AvalorAI/actify/blob/main/LICENSE
[docs-badge]: https://docs.rs/actify/badge.svg
[docs-url]: https://docs.rs/actify/latest/actify/

## Installation

```sh
cargo add actify
```

## Benefits

By generating the boilerplate code for you, a few key benefits are provided:

- Async actor model built on Tokio and channels, which can keep arbitrary owned data types.
- [Atomic](https://www.codingem.com/atomic-meaning-in-programming/) access and mutation of underlying data through clonable handles.
- Typed arguments and return values on the methods from your actor, exposed through each handle.
- No need to manually define message structs or enums!
- Built-in methods like `get()`, `set()`, `set_if_changed()`, and `subscribe()` even without using the macro.
- Automatic broadcasting of state changes to subscribers, with `#[skip_broadcast]` and `#[broadcast]` controls.
- Local synchronization through `Cache`, with `recv`, `recv_newest`, and non-blocking variants.
- Rate-limited updates through `Throttle` with configurable `Frequency`.
- Generic type parameters supported in both actor types and method arguments.
- Extension traits for common types: `Vec`, `Option`, `HashMap`, `HashSet`.

## Example

Consider the following example, in which you want to turn your custom Greeter into an actor:

```rust,no_run
use actify::{Handle, actify};

#[derive(Clone, std::fmt::Debug)]
struct Greeter {}

#[actify]
impl Greeter {
    fn say_hi(&self, name: String) -> String {
        format!("hi {}", name)
    }
}

#[tokio::main]
async fn main() {
    // An actify handle is created and initialized with the Greeter struct
    let handle = Handle::new(Greeter {});

    // The say_hi method is made available on its handle through the actify! macro
    let greeting = handle.say_hi("Alfred".to_string()).await;

    // The method is executed remotely on the initialized Greeter and returned through the handle
    assert_eq!(greeting, "hi Alfred".to_string())
}
```

## Reactive Subscriptions

Actors broadcast state changes automatically. Subscribers and caches let you react to updates without polling:

```rust,no_run
use actify::Handle;

#[tokio::main]
async fn main() {
    let handle = Handle::new(0);

    // Subscribe to raw broadcast events
    let mut rx = handle.subscribe();

    // Or create a local cache that stays in sync
    let mut cache = handle.create_cache().await;

    handle.set(42).await;

    // The subscriber receives every change
    assert_eq!(rx.recv().await.unwrap(), 42);

    // The cache provides synchronous access to the latest value
    assert_eq!(cache.get_newest(), &42);
}
```

For full API documentation, see [docs.rs](https://docs.rs/actify/latest/actify/).
