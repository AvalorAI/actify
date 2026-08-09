# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries before 0.8.3 were reconstructed from the git history after the fact, so
they record what changed rather than why, and are not exhaustive. 0.8.0 through
0.8.2 were released without tags; their dates are those of the version bump.

## [Unreleased]

### Added

- `OptionHandle::take` and `OptionHandle::replace`, mirroring
  `std::option::Option::take` and `std::option::Option::replace` in both
  signature and behaviour.

### Changed

- **Breaking:** methods taking `&self` no longer broadcast. Broadcasting follows
  the receiver: `&mut self` broadcasts, `&self` does not. Previously every
  method broadcast regardless of receiver, so read-only calls woke every
  subscriber, cache and throttle.

  This changes runtime behaviour without breaking compilation. A `&self` method
  that relied on broadcasting keeps compiling and stops broadcasting. Add
  `#[actify::broadcast]` to it to restore the old behaviour.

  `#[actify::skip_broadcast]` now applies only to `&mut self` methods, and
  `#[actify::broadcast]` only to `&self` methods. Applying either where it has
  no effect is a compile error naming the reason.

### Fixed

- Extension getters such as `VecHandle::is_empty` and `HashMapHandle::keys` no
  longer broadcast.

## [0.8.3] - 2026-08-08

Bug fixes, macro diagnostics and documentation. No API changes.

Requires `actify-macros` 0.5.0. Earlier releases resolved the macro crate from
crates.io while every test ran against the workspace copy, so the two could
drift; the dependency is now declared by path and version together.

### Fixed

- Method arguments named `s`, `args`, `res` or `result` no longer collide with
  identifiers in the generated code, which failed to compile with errors about
  the argument's own type.
- Impl blocks with inline bounds (`impl<T: Clone> Foo<T>`), `const` parameters,
  or a self type whose argument is not the bare parameter (`impl<T> Wrapper<Vec<T>>`)
  now generate valid code. The last case previously called a different type.
- `unsafe fn`, `self` by value, and return types that borrow the actor or use
  `impl Trait` are reported against the offending code instead of failing inside
  generated tokens.
- All macro errors in an impl block are reported in one compile, and the block
  is emitted alongside them, so its call sites no longer add "no method named"
  errors on top of the real diagnostic.
- Attributes named `broadcast` or `skip_broadcast` belonging to other crates are
  no longer consumed as actify's.
- `Handle::set_if_changed` broadcasts under its own name rather than `set`,
  which had merged the two counts under the `profiler` feature.
- A `Cache` now delivers a final update broadcast before its actor stopped
  instead of reporting the channel closed and dropping the value.
- Calls on a handle whose actor has stopped report whether it panicked or is no
  longer running. Both cases previously claimed a panic, including at runtime
  shutdown where none had occurred.
- Throttle callbacks receive the value without a clone per fire, which also
  lifts the `Clone` bound on the callback argument type.

### Changed

- Actor methods are typed `FnOnce`, removing the `futures` dependency along with
  a workaround for a limitation lifted in Rust 1.35.

### Documentation

- `missing_docs` is enabled in both crates, and everything it found is
  documented.
- Every method that reaches the actor carries a `# Panics` section.
- New crate sections on the execution model, including that two actors calling
  each other deadlock, and on actor lifetime and panics.
- Broadcasting is described as it behaves: it follows the attributes, not the
  receiver, and applies to `&self` methods.
- The README recommended `#[skip_broadcast]`, which does not compile unless
  imported; it now matches the rest of the documentation. Its examples are
  compiled and run as doctests.
- docs.rs builds with all features, so `profiler` items are published.

### Internal

- CI runs fmt, clippy, docs on default and all features, an MSRV check, a
  Windows test leg, a build without the `profiler` feature, and the compile-fail
  suite on a pinned toolchain.
- `rust-version = "1.85"` is declared.
- Task-lifecycle tests wait for conditions rather than fixed durations.

## 0.8.2 - 2026-08-04

### Fixed

- Updates broadcast while a `Cache` or `Throttle` was being created are no
  longer lost.

## 0.8.1 - 2026-03-12

### Added

- `HashMapHandle::keys`, `HashMapHandle::values`, `HashMapHandle::remove` and
  `HashMapHandle::clear`.

## 0.8.0 - 2026-02-21

### Added

- `BroadcastAs`, letting non-`Clone` actors broadcast a summary type, with the
  broadcast type as a second parameter on `Handle<T, V>`.
- `Handle::with` and `Handle::with_mut`, for reading or mutating part of an
  actor's value without cloning it.
- `#[actify(name = "...")]` for multiple impl blocks on one type, and
  `#[actify(skip_broadcast)]` with a per-method `#[actify::broadcast]` override.
- Blocking `Cache` receive methods, and `Cache` construction from a custom or
  default value.
- Macro support for arrays, tuples, const generics and destructured arguments.

### Changed

- Attributes are propagated to generated code from an allowlist, so proc-macro
  attributes such as `#[instrument]` stay on the original method.
- The crate was split into modules, and the actor trait was removed in favour of
  generated handle traits alone.

## [0.7.3] - 2025-09-18

### Added

- `ReadHandle::subscribe`.

## [0.7.2] - 2025-09-16

### Added

- `ReadHandle`, a read-only view of an actor.

### Changed

- `Handle` no longer requires `Debug`, and its own `Debug` output was improved.

## [0.7.0] - 2025-07-18

### Added

- `Handle::set_if_changed`.
- Throttles can be created from a `Cache`.

### Fixed

- Argument mutability is no longer carried into generated trait definitions.

## [0.6.1] - 2025-05-27

### Changed

- Removed the `async-trait` dependency.

## [0.6.0] - 2025-03-26

### Changed

- Throttle errors and the throttle builder were removed, along with the actor
  error type.

## [0.5.2] - 2025-03-08

### Changed

- Removed unnecessary mutability.

[0.8.3]: https://github.com/AvalorAI/actify/compare/0.7.3...v0.8.3
[0.7.3]: https://github.com/AvalorAI/actify/compare/0.7.2...0.7.3
[0.7.2]: https://github.com/AvalorAI/actify/compare/0.7.0...0.7.2
[0.7.0]: https://github.com/AvalorAI/actify/compare/v0.6.1...0.7.0
[0.6.1]: https://github.com/AvalorAI/actify/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/AvalorAI/actify/compare/v0.5.2...v0.6.0
[0.5.2]: https://github.com/AvalorAI/actify/compare/v0.5.1...v0.5.2
