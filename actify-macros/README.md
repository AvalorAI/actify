# actify-macros

The procedural macros behind [actify](https://crates.io/crates/actify). This crate
is not meant to be used on its own: the code it generates reaches into
`actify::__private`, so it only compiles as part of an actify build.

Add actify instead, which re-exports what you need:

```sh
cargo add actify
```

See the [actify documentation](https://docs.rs/actify/latest/actify/) for what the
`#[actify]` attribute generates and which method signatures it accepts.
