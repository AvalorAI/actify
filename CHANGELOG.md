# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries before 0.8.3 were reconstructed from the git history after the fact, so
they record what changed rather than why, and are not exhaustive. 0.8.0 through
0.8.2 were released without tags; their dates are those of the version bump.

## [Unreleased]

### Added

- `HashMapHandle` gains `len`, `contains_key`, `drain`, `extend`, `retain` and
  `get_or_insert_with`. `drain` returns `Vec<(K, V)>` rather than an iterator, and
  `get_or_insert_with` returns a clone of the value, since a reference cannot
  leave the actor.


- `VecDequeHandle` for `Handle<VecDeque<T>>`, with `push_back`, `push_front`,
  `pop_back`, `pop_front`, `front`, `back`, `get_index`, `len`, `is_empty`,
  `clear`, `contains`, `drain` and `retain`.

  `front`, `back` and `get_index` clone what `std` would borrow. `get_index`
  carries its suffix because an inherent `Handle::get` would shadow a generated
  method called `get`.


- `StringHandle` for `Handle<String>`, with `len`, `is_empty`, `clear`,
  `truncate`, `push`, `push_str`, `contains`, `starts_with`, `ends_with`,
  `replace`, `split`, `trim`, `to_lowercase` and `to_uppercase`.

  The methods that borrow in `std` return owned values here, since nothing
  borrowed can leave an actor: `trim` and `replace` return `String`, and `split`
  returns `Vec<String>` rather than an iterator. Arguments are owned for the same
  reason, so the pattern methods take `String`.


- `OptionHandle::take` and `OptionHandle::replace`, mirroring
  `std::option::Option::take` and `std::option::Option::replace` in both
  signature and behaviour.
- `Cache::clone_newest`, which reads the updates queued in a cache before
  cloning it, so both caches start from the newest broadcast value. A plain
  clone leaves those updates with the original.
- Async throttle callbacks: `Handle::spawn_async_throttle`,
  `Cache::spawn_async_throttle`, `Throttle::spawn_async_from_receiver` and
  `Throttle::spawn_async_interval`. Each call is awaited before the throttle
  looks for the next value, so a slow callback delays the following send rather
  than running alongside it.

  The callback borrows the client and returns a `BoxFuture`, built with
  `Box::pin`, so an `async fn` taking `&self` fits and the client is neither
  cloned nor required to be `Clone`. A future that borrows carries the lifetime
  of that borrow in its type, which a plain generic return type cannot express,
  hence the box. `BoxFuture` is exported for naming the bound.
- `Handle::wait_until`, `ReadHandle::wait_until` and `Cache::wait_until`, which
  wait until the broadcast value satisfies a predicate and return the value that
  satisfied it. The current value is tested first, so a predicate that already
  holds returns without waiting for an update.

  Every value is tested in the order it was broadcast, so a state the actor has
  since moved past still ends the wait. A lagging receiver is the one case where
  a matching value can be missed: it is logged and the wait continues.

  The predicate takes the broadcast type, which the actor produces without
  cloning itself, so waiting works on non-Clone actor types. `Handle::wait_until`
  panics if the actor stops while it waits, as the other handle methods do;
  `Cache::wait_until` returns `CacheRecvNewestError::Closed`.
- `ReadHandle::spawn_throttle` and `ReadHandle::spawn_async_throttle`, so a
  throttle can be spawned from a read-only view of an actor. Both behave as
  their `Handle` counterparts.
- `Throttle::abort` and `Throttle::is_finished`. A throttle spawned by
  `Throttle::spawn_interval` has no actor attached, so before this nothing could
  stop it short of shutting down the runtime.

### Removed

- **Breaking:** `Handle::new_throttled`. It was `Handle::new` followed by a
  throttle spawn, and it stopped composing once spawn functions began returning a
  `Throttle`: it would have had to hand back a tuple. Its one property worth
  keeping is that it needs no `await`, and the replacement does not either:

  ```rust
  let init = value.to_view();
  let handle = Handle::new(value);
  let throttle = Throttle::spawn(client, call, freq, handle.subscribe(), Some(init));
  ```

  Nothing can broadcast between `new` and `subscribe`, because no other handle to
  that actor exists yet. Note that the initial value is now passed explicitly, so
  it is on the caller to pass the value the actor was created with.

### Fixed

- Generated code no longer breaks when something in scope shares a name with what
  that code refers to. A user type called `Box` next to an `#[actify]` impl made
  the generated body resolve `Box::new` to that type, and a module named `actify`
  would have broken the paths to the crate itself. Generated code now names
  standard library types by absolute path (`::std::boxed::Box`) and reaches the
  crate as `::actify`, neither of which a local item can shadow.

### Changed

- **Breaking:** `Handle::send_job` is renamed to `Handle::__send_job`, and `Actor`
  moves from the crate root to `actify::__private`. Both were `#[doc(hidden)]`
  already and exist only for generated code to reach. The names now say so, and
  the module gives that contract one place to live rather than leaving it spread
  across a hidden root export and a hidden method.



- **Breaking:** the `Throttled` trait is gone. A throttle callback's argument type
  now comes from `ToView`, which the trait duplicated: both were a parameterized
  owned conversion from `&self` with a blanket implementation for `Clone` types.

  Replace `impl Throttled<Payload> for View` with `impl ToView<Payload> for View`
  and rename the method from `parse` to `to_view`. Bounds change from
  `V: Throttled<F>` to `V: ToView<F>`. A view type can carry several `ToView`
  implementations, and the callback signature selects which one is used, exactly
  as it did before.


- **Breaking:** renames, with no behaviour change and no deprecated aliases.

  | Before | After |
  | --- | --- |
  | `Handle::get_read_handle` | `Handle::read_handle` |
  | `Handle::create_cache` | `Handle::cache` |
  | `Handle::create_cache_from` | `Handle::cache_from` |
  | `Handle::create_cache_from_default` | `Handle::cache_from_default` |
  | `Handle::capacity` | `Handle::remaining_capacity` |
  | `Cache::get_current` | `Cache::current` |
  | `Cache::get_newest` | `Cache::newest` |
  | `Throttle::spawn_from_receiver` | `Throttle::spawn` |
  | `Throttle::spawn_async_from_receiver` | `Throttle::spawn_async` |

  The `get_` prefixes go because Rust getters do not carry one. `create_cache`
  loses its verb for the same reason, now that its neighbour `read_handle` has.
  `capacity` returned tokio's remaining slots rather than the configured size,
  so the name said the opposite of what it measured, and an inherent `capacity`
  also shadowed any actor method of that name.

  The `ReadHandle` counterparts of the cache constructors are renamed the same way.


- **Breaking:** `Frequency` no longer derives `PartialOrd` and `Ord`. The derived
  order came from the declaration order of the variants, so `OnEvent` compared
  less than `Interval`, which means nothing, and among intervals a longer
  duration compared greater while being the lower frequency. `PartialEq` and `Eq`
  stay.


- **Breaking:** `CacheRecvNewestError` is gone. Every receive on a `Cache`
  returns `CacheRecvError`. The methods that skip to the newest value never
  return `Lagged`, which each of them documents, so a caller of those gains a
  match arm that cannot be reached. Matching stays exhaustive.

- The `thiserror` dependency is dropped. `CacheRecvError` implements `Display`
  and `Error` by hand, with the same messages: `Cache channel closed` and
  `Cache channel lagged by {n}`.


- **Breaking:** `BroadcastAs<V>` is renamed to `ToView<V>` and `to_broadcast` to
  `to_view`, because the trait no longer decides only what is broadcast.

- **Breaking:** `Handle::get` and `ReadHandle::get` return the actor's view `V`
  rather than the actor type `T`, and no longer require `T: Clone`.

  `V` is now what a handle exposes: `get`, `subscribe`, `Cache`, `Throttle` and
  `wait_until` all speak it, while `with` and `with_mut` reach the actor type
  itself. Reading a value no longer clones the whole actor to derive a summary
  from the clone.

  **Nothing changes when `V = T`**, which is the default for every `Clone` actor
  type: `get` still returns a clone of the value, at the same cost. Actor types
  that are not `Clone` gain a `get`, which they did not have. Only actors with an
  explicit view type see a difference, and it is the view they asked for; the
  whole value is still available through `with(|state| state.clone())`.

  Most call sites that need updating fail to compile. The exception is a value
  passed to something generic, such as `log::info!("{:?}", handle.get().await)`
  or a serializer, which keeps compiling and starts reporting the view.


- `Handle::create_cache`, `Handle::spawn_throttle` and `Handle::spawn_async_throttle`,
  and their `ReadHandle` counterparts, no longer require the actor type to be
  `Clone`. Each of them seeded itself by cloning the whole actor value out with
  `get` and deriving the broadcast value from that clone. They now ask the actor
  to derive it in place, which drops the bound and the clone: a
  `Handle<BigState, Summary>` no longer copies all of `BigState` to create a
  cache, and actor types that are not `Clone` can now use both.


- **Breaking:** `Cache::spawn_throttle` takes `&mut self` and first
  synchronizes the cache to the newest broadcast value, which becomes the
  throttle's initial fire. Previously the throttle got a fresh subscription
  starting at the channel tail and fired with the stale snapshot, so updates
  already queued in the cache never reached it. The synchronization counts as
  receiving those updates: a later receive on the cache returns only updates
  broadcast after this call.

- **Breaking:** cloning a `Cache` now yields a fresh cache: the clone delivers
  the original's current value on its first read and receives broadcasts made
  after its creation. Previously a clone copied the original's first-read
  state, so a clone taken after that read delivered nothing until the next
  broadcast. Updates already queued in the original's receiver stay with the
  original in both the old and the new behaviour.

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

- A `Throttle` on `Frequency::Interval` no longer sends a burst when a send
  outlasts its interval. `tokio::time::interval` releases every tick that came
  due while the loop was busy, so one send lasting three intervals was followed
  by three sends in the same instant. The interval now skips the ticks it missed
  and stays on its original schedule.
- Both interval frequencies send the newest value available rather than the one
  that was current when the tick came due. An overdue tick can win the race
  against values already queued, and it previously sent the older value while a
  newer one sat unread in the channel.

- Extension getters such as `VecHandle::is_empty` and `HashMapHandle::keys` no
  longer broadcast.

- **Breaking:** throttle callbacks take any `Fn(&C, F) + Send + 'static` instead
  of a `fn(&C, F)` pointer, so a closure holding captured state is accepted.
  Method references and non-capturing closures still coerce, so existing call
  sites are unchanged. Affects `Throttle::spawn_from_receiver`,
  `Throttle::spawn_interval`, `Handle::spawn_throttle`, `Handle::new_throttled`
  and `Cache::spawn_throttle`. Code naming the old type explicitly, such as a
  struct field of type `fn(&C, F)`, needs the new parameter.
- **Breaking:** `Throttle` is now a handle to a running throttle rather than a
  generic configuration struct, so `Throttle<C, T, F>` becomes `Throttle`. Its
  spawn functions keep their names and arguments and gained the generics, so
  call sites are unchanged apart from the return value.
- **Breaking:** `Throttle::spawn_from_receiver`, `Throttle::spawn_interval`,
  `Handle::spawn_throttle` and `Cache::spawn_throttle` return a `Throttle`
  instead of `()`. Dropping it leaves the throttle running. `spawn_interval` is
  `#[must_use]`, since nothing else can stop that task.

### Documentation

- `Frequency::OnEventWhen` said it "fires for an event only after the interval
  has passed", which reads as a per-event delay. It sends at most once per
  interval, and only when a value arrived since the last send. The interval runs
  from startup and is not restarted by a send, so the wait after a value is
  anything from nothing to a full interval.

### Internal

- The throttle loop's interval, receiver and event bookkeeping moved into a
  private `ThrottleState`, which made the lagging-receiver path testable. It had
  never been executed by a test.
- `test_exit_on_shutdown` asserted `0 == 0`, because the throttle it built had
  no initial value and so never fired.

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
