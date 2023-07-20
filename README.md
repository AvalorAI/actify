# Actify

**Note that this crate is under construction. Although used in production, work is done on making an intuitive API, documententation and remaining features. For the time being, this does not follow semantic versioning!**

Actify is an [actor model](https://en.wikipedia.org/wiki/Actor_model) built on [Tokio](https://tokio.rs) that allows annotating any regular implementation block of your own type with the actify! macro.

[![Crates.io][crates-badge]][crates-url]
[![License][mit-badge]][mit-url]
[![Docs][docs-badge]][docs-url]

[crates-url]: https://crates.io/crates/actify
[crates-badge]: https://img.shields.io/crates/v/actify.svg
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: https://github.com/AvalorAI/actify/blob/main/LICENSE
[docs-badge]: https://docs.rs/actify/badge.svg
[docs-url]: https://docs.rs/actify/latest/actify/

## Benefits

By generating the boilerplate code for you, a few key benefits are provided:

- Async actor model build on Tokio and channels, which can keep arbitrary owned data types.
- [Atomic](https://www.codingem.com/atomic-meaning-in-programming/) access and mutation of underlying data through clonable handles.
- Typed arguments and return values on the methods from your actor, exposed through each handle.
- No need to manually define message structs or enums!
- Generic methods like get() and set() even without using the macro.

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
let greeting = handle.say_hi("Alfred".to_string()).await.unwrap();

// The method is executed remotely on the initialized Greeter and returned through the handle
assert_eq!(greeting, "hi Alfred".to_string())
}
```
