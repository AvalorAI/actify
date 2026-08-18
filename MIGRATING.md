# Migrating from 0.8 to 0.9

0.9.0 is a breaking release. Most of it is renames the compiler will point at,
but four changes compile cleanly and behave differently, so start there.

[CHANGELOG.md](CHANGELOG.md) records what changed and why; this file records what
to do about it.

## 1. Changes the compiler will not catch

### A `&self` method no longer broadcasts

Broadcasting follows the receiver. `&mut self` broadcasts, `&self` does not.
Before, every generated method broadcast, so a read woke every subscriber, cache
and throttle.

```rust
#[actify]
impl Counter {
    fn value(&self) -> i32 { self.value }        // broadcast in 0.8, silent in 0.9
    fn increment(&mut self) { self.value += 1 }  // broadcasts in both
}
```

Nothing fails to compile. If a `&self` method was relied on to notify
subscribers, which is meaningful when the type has interior mutability or a
`to_view` that reads more than the value, ask for it explicitly:

```rust
#[actify::broadcast]
fn value(&self) -> i32 { self.value }
```

The attributes are now checked against the receiver: `#[actify::broadcast]` on a
`&mut self` method and `#[actify::skip_broadcast]` on a `&self` method are
compile errors naming the reason, because neither does anything.

Extension methods follow the same rule, so `VecHandle::is_empty`,
`HashMapHandle::keys` and the other readers stopped broadcasting too.

### `Handle::get` returns the view

`get` returns `V`, the view type, rather than the actor type `T`.

**If you never named a view type, nothing changes.** `V` defaults to `T` for
every `Clone` actor, and `get` still returns a clone of the value at the same
cost. Actor types that are not `Clone` gain a `get` they did not have.

For `Handle<T, V>` with an explicit `V`, most call sites fail to compile and are
easy to fix. The one that does not is a value handed to something generic:

```rust
log::info!("{:?}", handle.get().await);   // now logs the view, not the actor
serde_json::to_string(&handle.get().await)?;
```

Reach the actor type with `with`:

```rust
handle.with(|state| state.clone()).await
```

### Cloning a `Cache` gives a fresh cache

A clone now delivers the current value of the original on its first read, then
receives broadcasts made after the clone. Before, it copied the first-read state
of the original, so a clone taken after that read returned nothing until the next
broadcast. In both versions the updates already queued in the receiver of the
original stay there; `Cache::clone_newest` reads them first, so both caches start
from the newest value.

### Throttle spawns return a handle

`Throttle::spawn`, `Throttle::spawn_interval`, `Handle::spawn_throttle` and
`Cache::spawn_throttle` return a `Throttle` instead of `()`. Dropping it leaves
the throttle running, so existing call sites keep working. The two interval
spawns are `#[must_use]`, since nothing else can stop those tasks.

Two throttle behaviours also changed on their own: an interval no longer emits a
burst of catch-up ticks after a send outlasts its interval, and both interval
frequencies now send the newest value available rather than the one that was
current when the tick came due.

## 2. Renames

No behaviour change, no deprecated aliases.

| 0.8 | 0.9 |
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
| `BroadcastAs<V>` | `ToView<V>` |
| `BroadcastAs::to_broadcast` | `ToView::to_view` |

The `ReadHandle` cache constructors are renamed the same way.

`capacity` became `remaining_capacity` because it returns the remaining slots in
the tokio channel rather than the configured size, so the old name said the
opposite of what it measured. It also shadowed any actor method called
`capacity`.

## 3. `Throttled` is gone, use `ToView`

`Throttled<F>` and `BroadcastAs<V>` were the same shape: a parameterized owned
conversion from `&self` with a blanket implementation for `Clone` types. Only
`ToView` remains.

```rust
// 0.8
impl Throttled<Payload> for View {
    fn parse(&self) -> Payload { todo!() }
}

// 0.9
impl ToView<Payload> for View {
    fn to_view(&self) -> Payload { todo!() }
}
```

Bounds change from `V: Throttled<F>` to `V: ToView<F>`. A view type can still
carry several `ToView` implementations, and the callback signature selects which
one is used.

## 4. One cache error

`CacheRecvNewestError` is gone. Every receive on a `Cache` returns
`CacheRecvError`.

```rust
// 0.8
match cache.recv_newest().await {
    Err(CacheRecvNewestError::Closed) => handle_shutdown(),
    Ok(value) => use_it(value),
}

// 0.9
match cache.recv_newest().await {
    Err(CacheRecvError::Closed) => handle_shutdown(),
    Err(CacheRecvError::Lagged(_)) => unreachable!("recv_newest skips to the newest value"),
    Ok(value) => use_it(value),
}
```

The `*_newest` methods and `wait_until` never return `Lagged`, which each of them
documents, so a caller of those gains an arm that cannot be reached. Matching
stays exhaustive: `CacheRecvError` is not `#[non_exhaustive]`.

`Frequency` no longer derives `PartialOrd` and `Ord`. The derived order came from
the declaration order of the variants, so `OnEvent` compared less than
`Interval`, and among intervals a longer duration compared greater while being
the lower frequency. `PartialEq` and `Eq` stay.

## 5. Throttle types and callbacks

`Throttle<C, T, F>` is now plain `Throttle`, a handle to a running throttle
rather than a configuration struct. The spawn functions keep their names and
arguments and carry the generics themselves, so only a stored type needs
changing.

Callbacks take any `Fn(&C, F) + Send + 'static` rather than a `fn(&C, F)`
pointer, so a closure with captured state is now accepted. Method references and
non-capturing closures still coerce. Only code naming the old type explicitly,
such as a struct field of type `fn(&C, F)`, needs an update.

`Handle::new_throttled` is removed. It was `new` followed by a throttle spawn,
and it stopped composing once spawns began returning a `Throttle`. The
replacement needs no `await` either:

```rust
let init = value.to_view();
let handle = Handle::new(value);
let throttle = Throttle::spawn(client, call, freq, handle.subscribe(), Some(init));
```

Nothing can broadcast between `new` and `subscribe`, because no other handle to
that actor exists yet. The initial value is now passed explicitly, so it is on
the caller to pass the value the actor was created with.

`Cache::spawn_throttle` takes `&mut self` and synchronizes the cache to the
newest broadcast value first, which becomes the initial fire of the throttle.
That synchronization counts as receiving those updates, so a later receive on the
cache returns only what was broadcast after the call.

## 6. Generated traits promise a `Send` future

The generated handle traits declare methods as
`fn method(..) -> impl Future<Output = R> + Send` rather than `async fn`, so code
generic over a handle trait can spawn the call.

Hand-written implementations, such as test stand-ins, keep compiling: an
`async fn` satisfies the new signature as long as its future is `Send`. One that
holds an `Rc` or a `RefCell` guard across an await now fails.

The generated implementation also bounds the broadcast type, so code generic over
it without bounds needs them:

```rust
// 0.8
async fn read<V>(handle: Handle<Counter, V>) -> i32 { handle.value().await }

// 0.9
async fn read<V: Send + Sync + 'static>(handle: Handle<Counter, V>) -> i32 { handle.value().await }
```

Calls on a concrete handle are unaffected, because `Handle::new` already requires
those bounds.

## 7. tokio features

actify asks tokio for `macros`, `rt`, `sync` and `time` instead of `full`, which
takes eleven crates out of the dependency tree.

Cargo unions features across a dependency graph, so code that used `tokio::fs`,
`tokio::net` or another module without listing the feature in its own manifest
was relying on actify to enable it. Name the feature yourself:

```toml
tokio = { version = "1", features = ["fs", "net", "macros", "rt-multi-thread"] }
```

## 8. Macro internals

`Handle::send_job` is now `Handle::__send_job`, and `Actor` moved from the crate
root to `actify::__private`. Both were `#[doc(hidden)]` and exist only for
generated code to reach, so this affects nothing that was not already reaching
into internals. Recompiling against the 0.9 macro is enough.
