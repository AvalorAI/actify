//! This workspace is used to test the functionalities of actify as would any user that imports the library

use actify::{BroadcastAs, Handle, actify};
use std::{collections::HashMap, fmt::Debug, sync::Mutex};

fn main() {}

/// An example struct for the macro tests
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct TestStruct<T> {
    inner_data: T,
}

#[actify]
impl<T> TestStruct<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    fn foo(&mut self, i: i32, _h: HashMap<String, T>) -> f64 {
        (i + 1) as f64
    }

    fn bar<F>(&self, i: usize, f: F) -> usize
    where
        F: Fn(usize) -> usize + Send + Sync + 'static,
    {
        f(i)
    }

    #[actify::skip_broadcast]
    async fn baz(&mut self, i: i32) -> f64 {
        (i + 2) as f64
    }

    fn mut_test(&self, mut arg: String) {
        println!("{arg}");
        arg = "mutated".to_string();
        println!("{arg}")
    }
}

/// An example struct for the macro tests
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct SomeStruct {
    inner_bool: bool,
}

#[actify]
impl SomeStruct {
    fn set_true(&mut self) {
        self.inner_bool = true
    }

    fn set_false(&mut self) {
        self.inner_bool = false
    }
}

#[actify(name = "SomeStructGetters", skip_broadcast)]
impl SomeStruct {
    fn get_inner(&self) -> bool {
        self.inner_bool
    }
}

#[allow(dead_code)]
/// Example Extension trait
trait TestExt<T> {
    fn extended_foo(&mut self, i: i32, _h: HashMap<String, T>) -> f64;

    fn extended_bar<F>(&mut self, i: usize, f: F) -> usize
    where
        F: Fn(usize) -> usize + Send + Sync + 'static;
}

impl<T> TestExt<T> for TestStruct<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    fn extended_foo(&mut self, i: i32, _h: HashMap<String, T>) -> f64 {
        (i + 1) as f64
    }

    fn extended_bar<F>(&mut self, i: usize, f: F) -> usize
    where
        F: Fn(usize) -> usize + Send + Sync + 'static,
    {
        f(i)
    }
}

#[allow(dead_code)]
/// Example async Extension trait
trait AsyncTestExt<T> {
    async fn extended_baz(&mut self, i: i32) -> f64;
}

impl<T> AsyncTestExt<T> for TestStruct<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    async fn extended_baz(&mut self, i: i32) -> f64 {
        (i + 2) as f64
    }
}

#[allow(dead_code)]
#[derive(Clone)]
struct NonDebug;

#[actify]
impl NonDebug {
    fn foo(&self) {}
}

/// Argument names that collide with identifiers used in the generated code
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct ShadowingActor {
    value: String,
}

#[actify]
impl ShadowingActor {
    fn store(&mut self, s: String, args: Vec<i32>, res: u8, result: bool) -> String {
        self.value = format!("{s}-{args:?}-{res}-{result}");
        self.value.clone()
    }
}

/// Generic bounds written inline on the impl block instead of in a where clause
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct InlineBounds<T> {
    value: T,
}

#[actify]
impl<T: Clone + Debug + Send + Sync + 'static> InlineBounds<T> {
    fn get_value(&self) -> T {
        self.value.clone()
    }
}

/// A self type whose generic argument is not the bare type parameter
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct Wrapper<C> {
    items: C,
}

#[actify]
impl<T> Wrapper<Vec<T>>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    fn first_item(&self) -> Option<T> {
        self.items.first().cloned()
    }
}

/// A const generic parameter on the impl block
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct ConstActor<const N: usize> {
    data: [u8; N],
}

#[actify]
impl<const N: usize> ConstActor<N> {
    fn slots(&self) -> usize {
        N
    }
}

#[derive(Clone, Debug)]
struct ComplexActorTypes;

#[actify]
impl ComplexActorTypes {
    fn with_array(&self, data: [u8; 4]) -> u8 {
        data[0]
    }

    fn with_tuple(&self, pair: (String, i32)) -> String {
        format!("{}: {}", pair.0, pair.1)
    }

    fn with_fn_ptr(&self, f: fn(usize) -> usize, val: usize) -> usize {
        f(val)
    }

    fn with_multi_generic<A, B>(&self, a: A, b: B) -> (A, B)
    where
        A: Send + Sync + 'static,
        B: Send + Sync + 'static,
    {
        (a, b)
    }

    fn with_trait_object(&self, handler: Box<dyn Fn(i32) -> i32 + Send + Sync>) -> i32 {
        handler(42)
    }

    async fn async_generic<F>(&self, f: F) -> usize
    where
        F: Fn(usize) -> usize + Send + Sync + 'static,
    {
        f(42)
    }

    fn with_const_generic<const N: usize>(&self, arr: [u8; N]) -> usize
    where
        [u8; N]: Send + Sync + 'static,
    {
        arr.iter().map(|b| *b as usize).sum()
    }

    fn with_const_generic_and_type<T, const N: usize>(&self, _arr: [T; N]) -> usize
    where
        T: Send + Sync + 'static,
        [T; N]: Send + Sync + 'static,
    {
        N
    }

    fn with_destructure(&self, (a, b): (i32, i32)) -> i32 {
        a + b
    }

    fn with_mixed_destructure(&self, label: String, (x, y): (f64, f64)) -> String {
        format!("{}: ({}, {})", label, x, y)
    }
}

#[derive(Clone, Debug)]
struct AttributeTestActor;

#[allow(unused_variables)]
#[actify]
impl AttributeTestActor {
    /// Doc attribute propagated to handle trait
    fn with_doc(&self, x: i32) -> i32 {
        x
    }

    #[allow(unused_variables)]
    fn with_allow(&self, x: i32) -> i32 {
        42
    }

    #[allow(deprecated)]
    #[deprecated(note = "use with_doc instead")]
    fn with_deprecated(&self, x: i32) -> i32 {
        x
    }

    #[must_use]
    fn with_must_use(&self, x: i32) -> i32 {
        x + 1
    }

    #[cfg_attr(test, allow(unused_variables))]
    fn with_cfg_attr(&self, x: i32) -> i32 {
        42
    }

    #[cfg(target_os = "linux")]
    fn some_os_specific_method(&mut self) -> f64 {
        1.
    }

    #[cfg(target_os = "windows")]
    fn some_os_specific_method(&mut self) -> f64 {
        2.
    }
}

/// Tests that impl-block-level #[cfg] propagates to all generated traits and impls.
/// Without this, the handle trait impl and actor trait/impl would exist on the
/// wrong platform, referencing a trait that doesn't exist.
#[derive(Clone, Debug)]
struct CfgImplActor;

#[actify]
#[cfg(target_os = "linux")]
impl CfgImplActor {
    fn platform_value(&self) -> &'static str {
        "linux"
    }
}

#[actify]
#[cfg(target_os = "windows")]
impl CfgImplActor {
    fn platform_value(&self) -> &'static str {
        "windows"
    }
}

#[derive(Clone, Debug)]
struct SkipMultipleBroadcastsActor {
    value: i32,
}

#[actify(skip_broadcast)]
impl SkipMultipleBroadcastsActor {
    fn skipped_method(&mut self, x: i32) -> i32 {
        self.value = x;
        x
    }

    #[actify::broadcast]
    fn broadcast_method(&mut self, x: i32) -> i32 {
        self.value = x;
        x * 2
    }
}

/// Interior mutability lets a `&self` method change what subscribers observe,
/// which is the case `#[actify::broadcast]` exists for.
#[derive(Debug)]
struct InteriorMutabilityActor {
    value: Mutex<i32>,
}

impl BroadcastAs<i32> for InteriorMutabilityActor {
    fn to_broadcast(&self) -> i32 {
        *self.value.lock().unwrap()
    }
}

#[actify]
impl InteriorMutabilityActor {
    #[actify::broadcast]
    fn increment(&self) -> i32 {
        let mut value = self.value.lock().unwrap();
        *value += 1;
        *value
    }

    fn peek(&self) -> i32 {
        *self.value.lock().unwrap()
    }
}

/// Tests that proc-macro attributes like `#[instrument]` are stripped from generated
#[derive(Clone, Debug)]
struct InstrumentedActor {
    value: i32,
}

#[actify]
impl InstrumentedActor {
    #[tracing::instrument(skip_all)]
    fn get_value(&self) -> i32 {
        self.value
    }

    #[tracing::instrument(level = "debug", skip_all, fields(new_value))]
    fn set_value(&mut self, new_value: i32) {
        self.value = new_value;
    }

    /// Doc + instrument combined — doc should propagate, instrument should not.
    #[tracing::instrument(skip_all)]
    async fn async_get(&self) -> i32 {
        self.value
    }

    #[tracing::instrument(skip_all)]
    #[allow(unused_variables)]
    fn unused_set(&mut self, v: i32) {
        self.value = v;
    }
}

/// Same test with unqualified `instrument` (via `use tracing::instrument`).
#[derive(Clone, Debug)]
struct UnqualifiedInstrumentActor {
    count: u32,
}

#[allow(unused_imports)]
use tracing::instrument;

#[actify]
impl UnqualifiedInstrumentActor {
    #[instrument(skip_all)]
    fn increment(&mut self) -> u32 {
        self.count += 1;
        self.count
    }

    #[instrument(level = "trace", skip_all)]
    fn get_count(&self) -> u32 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actify::{Frequency, Handle, Throttle, VecHandle};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::time::{Instant, sleep};

    #[tokio::test]
    async fn test_custom_trait_name() {
        let handle = Handle::new(SomeStruct { inner_bool: false });

        // UFCS — verifies the generated trait names are correct
        SomeStructHandle::set_true(&handle).await;
        assert!(SomeStructGetters::get_inner(&handle).await);

        // Method-call syntax — verifies both traits resolve without ambiguity
        handle.set_false().await;
        assert!(!handle.get_inner().await);
    }

    // NOTE: "should not compile" tests live in tests/compile_fail/ and are run via trybuild.
    // A compile_error! from the macro fires at compile time, so it cannot be tested inline.

    #[tokio::test]
    async fn test_complex_arg_types() {
        let handle = Handle::new(ComplexActorTypes);

        assert_eq!(handle.with_array([10, 20, 30, 40]).await, 10);
        assert_eq!(
            handle.with_tuple(("hello".to_string(), 42)).await,
            "hello: 42"
        );
        assert_eq!(handle.with_fn_ptr(|x| x * 2, 21).await, 42);
        assert_eq!(
            handle.with_multi_generic(42u32, "hello".to_string()).await,
            (42u32, "hello".to_string())
        );
        assert_eq!(handle.with_trait_object(Box::new(|x| x * 3)).await, 126);
        assert_eq!(handle.async_generic(|x| x + 8).await, 50);
        assert_eq!(handle.with_const_generic([1u8, 2, 3, 4]).await, 10);
        assert_eq!(handle.with_const_generic([10u8, 20]).await, 30);
        assert_eq!(handle.with_const_generic_and_type([1u32, 2, 3]).await, 3);
        assert_eq!(handle.with_const_generic_and_type(["a", "b"]).await, 2);
        assert_eq!(handle.with_destructure((3, 7)).await, 10);
        assert_eq!(
            handle
                .with_mixed_destructure("point".to_string(), (1.5, 2.5))
                .await,
            "point: (1.5, 2.5)"
        );
    }

    #[tokio::test]
    async fn test_attribute_propagation() {
        let handle = Handle::new(AttributeTestActor);

        // #[doc] — just needs to compile (docs propagated to handle trait)
        assert_eq!(handle.with_doc(5).await, 5);

        // #[allow(unused_variables)] — no warning despite unused x
        assert_eq!(handle.with_allow(99).await, 42);

        // #[deprecated] — propagated to handle trait, suppressed here
        #[allow(deprecated)]
        let result = handle.with_deprecated(10).await;
        assert_eq!(result, 10);

        // #[must_use] — propagated to handle trait, but has no effect on async fns
        // (Rust's #[must_use] on async fn warns about the unused Future, not the
        // resolved value, and .await always "uses" the Future.)
        assert_eq!(handle.with_must_use(5).await, 6);

        // #[cfg_attr(test, allow(unused_variables))] — conditional attribute
        assert_eq!(handle.with_cfg_attr(99).await, 42);

        // #[cfg] — OS-specific method, only one variant compiles
        #[cfg(target_os = "linux")]
        assert_eq!(handle.some_os_specific_method().await, 1.);
        #[cfg(target_os = "windows")]
        assert_eq!(handle.some_os_specific_method().await, 2.);

        // #[cfg] on impl block — all generated traits/impls must be gated
        let cfg_handle = Handle::new(CfgImplActor);
        #[cfg(target_os = "linux")]
        assert_eq!(cfg_handle.platform_value().await, "linux");
        #[cfg(target_os = "windows")]
        assert_eq!(cfg_handle.platform_value().await, "windows");
    }

    #[tokio::test]
    async fn test_macro() {
        let actor_handle = Handle::new(TestStruct {
            inner_data: "Test".to_string(),
        });

        assert_eq!(actor_handle.foo(0, HashMap::new()).await, 1.);
        assert_eq!(actor_handle.bar(5, |i: usize| i + 10).await, 15);
        assert_eq!(actor_handle.baz(0).await, 2.);
    }

    /// Bounds written inline on the impl block must not leak into the generated
    /// trait reference or the call expression, where they are not valid syntax
    #[tokio::test]
    async fn test_inline_generic_bounds() {
        let handle = Handle::new(InlineBounds { value: 7_i32 });
        assert_eq!(handle.get_value().await, 7);
    }

    /// The call must go through the full self type: `Wrapper<Vec<T>>`, not
    /// `Wrapper<T>` reconstructed from the impl block's generic parameters
    #[tokio::test]
    async fn test_non_identity_generic_argument() {
        let handle = Handle::new(Wrapper {
            items: vec![1_u8, 2, 3],
        });
        assert_eq!(handle.first_item().await, Some(1));
    }

    /// A `&'static` return outlives the actor, so it must keep working even
    /// though borrows of the actor's own state are rejected
    #[tokio::test]
    async fn test_static_reference_return() {
        let handle = Handle::new(CfgImplActor);
        assert!(!handle.platform_value().await.is_empty());
    }

    #[tokio::test]
    async fn test_impl_level_const_generic() {
        let handle = Handle::new(ConstActor { data: [0_u8; 4] });
        assert_eq!(handle.slots().await, 4);
    }

    /// Argument names like `s`, `args`, `res` and `result` must not clash with
    /// the identifiers the macro uses internally in the generated method body
    #[tokio::test]
    async fn test_shadowing_arg_names() {
        let handle = Handle::new(ShadowingActor {
            value: String::new(),
        });

        let stored = handle.store("x".to_string(), vec![1, 2], 3, true).await;
        assert_eq!(stored, "x-[1, 2]-3-true");
    }

    /// A cache does not keep its actor alive: it only receives broadcasts, so
    /// the actor still stops once the last handle goes out of scope.
    #[tokio::test]
    async fn test_handle_out_of_scope() {
        let baseline = alive_tasks();
        let handle_1 = Handle::new(1);

        let mut cache_3 = {
            let _handle_2 = Handle::new("test");
            let handle_3 = Handle::new(1.); // This goes out of scope
            let _handle_1_clone = handle_1.clone();
            handle_3.create_cache().await // But the cache doesn't
        };

        // Only handle_1's actor survives the scope, even though cache_3 does
        let remaining = await_alive_tasks(baseline + 1).await;
        assert_eq!(
            remaining,
            baseline + 1,
            "expected only handle_1's actor to still be running"
        );

        // Its broadcast channel is closed, so the cache can no longer receive
        assert!(cache_3.try_recv_newest().is_err());
    }

    /// An actor runs one job at a time, so two actors calling each other each
    /// wait for a reply the other cannot produce yet. The crate docs state
    /// this; the test keeps that statement true.
    #[tokio::test(start_paused = true)]
    async fn test_actors_calling_each_other_never_complete() {
        let parser = Handle::new(Parser { store: None });
        let store = Handle::new(Store { parser: None });

        parser
            .set(Parser {
                store: Some(store.clone()),
            })
            .await;
        store
            .set(Store {
                parser: Some(parser.clone()),
            })
            .await;

        assert!(
            never_resolves(parser.parse()).await,
            "the cycle returned instead of blocking"
        );
    }

    #[tokio::test]
    async fn test_drain_vec() {
        let actor_handle = Handle::new(vec![1, 2, 3]);

        assert_eq!(actor_handle.drain(1..).await, vec![2, 3]);
        assert_eq!(actor_handle.get().await, vec![1]);
    }

    #[tokio::test]
    async fn test_skip_broadcast() {
        let actor_handle = Handle::new(TestStruct {
            inner_data: "Test".to_string(),
        });

        let mut rx = actor_handle.subscribe();
        assert!(rx.try_recv().is_err()); // Nothing

        actor_handle.foo(0, HashMap::new()).await;
        assert!(rx.try_recv().is_ok());

        actor_handle.foo(1, HashMap::new()).await;
        assert!(rx.try_recv().is_ok());

        actor_handle
            .set(TestStruct {
                inner_data: "Test2".to_string(),
            })
            .await;
        assert!(rx.try_recv().is_ok());

        actor_handle.baz(0).await;
        assert!(rx.try_recv().is_err()); // Nothing

        let counts = actify::get_broadcast_counts();
        println!("{:?}", counts);

        let sorted_counts = actify::get_sorted_broadcast_counts();
        println!("{:?}", sorted_counts);
    }

    #[tokio::test]
    async fn test_instrument_attr_stripped_from_handle() {
        let handle = Handle::new(InstrumentedActor { value: 10 });

        assert_eq!(handle.get_value().await, 10);
        handle.set_value(42).await;
        assert_eq!(handle.get_value().await, 42);
        assert_eq!(handle.async_get().await, 42);
    }

    #[tokio::test]
    async fn test_unqualified_instrument_attr() {
        // Same test with unqualified `#[instrument]` (single-segment path)
        let handle = Handle::new(UnqualifiedInstrumentActor { count: 0 });

        assert_eq!(handle.increment().await, 1);
        assert_eq!(handle.increment().await, 2);
        assert_eq!(handle.get_count().await, 2);
    }

    #[tokio::test]
    async fn test_block_skip_broadcast() {
        let handle = Handle::new(SkipMultipleBroadcastsActor { value: 0 });
        let mut rx = handle.subscribe();

        // skipped_method has no #[broadcast], so block default (skip) applies
        handle.skipped_method(10).await;
        assert!(rx.try_recv().is_err());

        // broadcast_method has #[broadcast], overriding the block default
        handle.broadcast_method(20).await;
        assert!(rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn test_ref_self_broadcast_opt_in() {
        let handle: Handle<InteriorMutabilityActor, i32> = Handle::new(InteriorMutabilityActor {
            value: Mutex::new(0),
        });
        let mut rx = handle.subscribe();

        assert_eq!(handle.peek().await, 0);
        assert!(rx.try_recv().is_err());

        assert_eq!(handle.increment().await, 1);
        assert_eq!(rx.try_recv().unwrap(), 1);
    }

    #[allow(dead_code)]
    pub fn load_logger() {
        env_logger::Builder::new()
            .filter(None, log::LevelFilter::Info)
            .init();
    }

    /// Helper to get current number of alive tasks in the runtime
    /// Returns whether a future is still pending once nothing else can make
    /// progress.
    ///
    /// The tests using this run on a paused clock, where tokio advances time
    /// as soon as every task is idle, so the timeout elapses immediately and
    /// its length is irrelevant.
    async fn never_resolves<F: std::future::Future>(future: F) -> bool {
        tokio::time::timeout(Duration::from_secs(1), future)
            .await
            .is_err()
    }

    fn alive_tasks() -> usize {
        tokio::runtime::Handle::current()
            .metrics()
            .num_alive_tasks()
    }

    /// Waits until the runtime reports `expected` alive tasks, or gives up.
    ///
    /// Tasks stop asynchronously: dropping a handle closes a channel, and the
    /// task only notices the next time it is scheduled. Sleeping for a fixed
    /// duration encodes a guess about how long that takes, which is what fails
    /// on a loaded machine. Polling returns as soon as the count settles and
    /// spends the whole deadline only when something is actually wrong, so the
    /// deadline can be generous without slowing the suite down.
    ///
    /// Returns the last observed count so the caller can assert on it and
    /// report the mismatch itself.
    async fn await_alive_tasks(expected: usize) -> usize {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let alive = alive_tasks();
            if alive == expected || Instant::now() >= deadline {
                return alive;
            }
            sleep(Duration::from_millis(5)).await;
        }
    }

    /// Gives tasks a chance to react, then reports the count.
    ///
    /// For assertions that a task keeps running, where polling for the
    /// expected value would return immediately and prove nothing. Waiting too
    /// briefly here can only make the test more lenient, never flaky.
    async fn settled_alive_tasks() -> usize {
        sleep(Duration::from_millis(50)).await;
        alive_tasks()
    }

    /// Helper struct for throttle testing
    #[derive(Debug, Clone)]
    struct TestClient {
        count: Arc<Mutex<i32>>,
    }

    impl TestClient {
        fn new() -> Self {
            TestClient {
                count: Arc::new(Mutex::new(0)),
            }
        }

        fn call(&self, _event: i32) {
            let mut count = self.count.lock().unwrap();
            *count += 1;
        }

        /// Waits until the callback has fired at least `expected` times.
        ///
        /// Polls rather than sleeping for a duration long enough to fit that
        /// many intervals, which would be a bet on the machine keeping up.
        async fn await_count(&self, expected: i32) -> i32 {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let count = *self.count.lock().unwrap();
                if count >= expected || Instant::now() >= deadline {
                    return count;
                }
                sleep(Duration::from_millis(5)).await;
            }
        }
    }

    #[tokio::test]
    async fn test_handle_task_cleanup() {
        // Record baseline task count
        let baseline = alive_tasks();

        // Creating a Handle spawns a Listener task
        let handle = Handle::new(42);

        let with_handle = await_alive_tasks(baseline + 1).await;
        assert!(
            with_handle > baseline,
            "Expected task count to increase after creating Handle. Baseline: {}, After: {}",
            baseline,
            with_handle
        );

        // Drop the handle - this should cause the Listener task to exit
        drop(handle);

        let after_drop = await_alive_tasks(baseline).await;
        assert_eq!(
            after_drop, baseline,
            "Expected task count to return to baseline after dropping Handle. Baseline: {}, After drop: {}",
            baseline, after_drop
        );
    }

    #[tokio::test]
    async fn test_handle_clone_task_cleanup() {
        // Record baseline task count
        let baseline = alive_tasks();

        // Creating a Handle spawns a Listener task
        let handle = Handle::new(42);
        let handle_clone = handle.clone();

        let with_handles = await_alive_tasks(baseline + 1).await;
        // Only one task should be spawned regardless of clones
        assert_eq!(
            with_handles,
            baseline + 1,
            "Expected exactly one task for Handle and its clone. Baseline: {}, After: {}",
            baseline,
            with_handles
        );

        // Dropping one clone shouldn't affect the task
        drop(handle);

        let after_first_drop = settled_alive_tasks().await;
        assert_eq!(
            after_first_drop,
            baseline + 1,
            "Task should still be running after dropping one clone. Baseline: {}, After: {}",
            baseline,
            after_first_drop
        );

        // Dropping the last clone should cause the task to exit
        drop(handle_clone);

        let after_all_drop = await_alive_tasks(baseline).await;
        assert_eq!(
            after_all_drop, baseline,
            "Task should exit after all Handle clones are dropped. Baseline: {}, After: {}",
            baseline, after_all_drop
        );
    }

    #[tokio::test]
    async fn test_throttle_from_receiver_task_cleanup() {
        // Record baseline task count
        let baseline = alive_tasks();

        // Create a Handle (spawns 1 task)
        let handle = Handle::new(1);

        let with_handle = await_alive_tasks(baseline + 1).await;
        assert_eq!(with_handle, baseline + 1, "Expected one task for Handle");

        // Spawn a throttle from the handle's receiver (spawns another task)
        let client = TestClient::new();
        let receiver = handle.subscribe();
        Throttle::spawn_from_receiver(
            client.clone(),
            TestClient::call,
            Frequency::Interval(Duration::from_millis(50)),
            receiver,
            Some(1),
        );

        let with_throttle = await_alive_tasks(baseline + 2).await;
        assert_eq!(
            with_throttle,
            baseline + 2,
            "Expected two tasks: Handle + Throttle. Baseline: {}, After: {}",
            baseline,
            with_throttle
        );

        // Dropping the handle should cause both tasks to exit:
        // - The Handle's Listener task exits because the channel closes
        // - The Throttle task exits because the broadcast receiver closes
        drop(handle);

        let after_drop = await_alive_tasks(baseline).await;
        assert_eq!(
            after_drop, baseline,
            "Both tasks should exit after Handle is dropped. Baseline: {}, After: {}",
            baseline, after_drop
        );
    }

    #[tokio::test]
    async fn test_throttle_spawn_interval_no_cleanup() {
        // Note: spawn_interval creates a Throttle without a receiver,
        // so it will run forever (until the runtime shuts down).
        // This test documents that behavior.

        let baseline = alive_tasks();

        let client = TestClient::new();
        Throttle::spawn_interval(
            client.clone(),
            TestClient::call,
            Duration::from_millis(50),
            1,
        );

        let with_throttle = await_alive_tasks(baseline + 1).await;
        assert_eq!(
            with_throttle,
            baseline + 1,
            "Expected one task for interval Throttle"
        );

        // Verify the throttle keeps firing on its interval
        let count = client.await_count(2).await;
        assert!(
            count >= 2,
            "Interval throttle should have fired repeatedly, count: {count}"
        );

        // Note: There's no way to stop an interval-based Throttle without a receiver.
        // This is expected behavior - the task will run until the runtime exits.
        // The task count will remain elevated.
    }

    #[tokio::test]
    async fn test_multiple_handles_task_cleanup() {
        let baseline = alive_tasks();

        // Create multiple independent handles
        let handle1 = Handle::new(1);
        let handle2 = Handle::new("test");
        let handle3 = Handle::new(1.5f64);

        let with_handles = await_alive_tasks(baseline + 3).await;
        assert_eq!(
            with_handles,
            baseline + 3,
            "Expected three tasks for three Handles"
        );

        // Drop them one by one and verify cleanup
        drop(handle1);
        assert_eq!(await_alive_tasks(baseline + 2).await, baseline + 2);

        drop(handle2);
        assert_eq!(await_alive_tasks(baseline + 1).await, baseline + 1);

        drop(handle3);
        assert_eq!(await_alive_tasks(baseline).await, baseline);
    }

    #[tokio::test]
    async fn test_cache_does_not_spawn_tasks() {
        let baseline = alive_tasks();

        let handle = Handle::new(42);

        let with_handle = await_alive_tasks(baseline + 1).await;
        assert_eq!(with_handle, baseline + 1, "Expected one task for Handle");

        // Creating a cache should NOT spawn additional tasks
        let _cache = handle.create_cache().await;

        let with_cache = settled_alive_tasks().await;
        assert_eq!(
            with_cache,
            baseline + 1,
            "Cache should not spawn additional tasks"
        );

        // Creating more caches still shouldn't spawn tasks
        let _cache2 = handle.create_cache().await;
        let _cache3 = handle.create_cache_from_default();

        let with_more_caches = settled_alive_tasks().await;
        assert_eq!(
            with_more_caches,
            baseline + 1,
            "Multiple caches should not spawn additional tasks"
        );
    }

    #[tokio::test]
    async fn test_cache_spawn_throttle_task_cleanup() {
        let baseline = alive_tasks();

        let handle = Handle::new(42);
        let cache = handle.create_cache().await;

        let with_handle = await_alive_tasks(baseline + 1).await;
        assert_eq!(with_handle, baseline + 1, "Expected one task for Handle");

        // Spawning a throttle from cache spawns a new task
        let client = TestClient::new();
        cache.spawn_throttle(client.clone(), TestClient::call, Frequency::OnEvent);

        let with_throttle = await_alive_tasks(baseline + 2).await;
        assert_eq!(
            with_throttle,
            baseline + 2,
            "Expected two tasks: Handle + Throttle"
        );

        // Dropping the cache doesn't affect tasks (it doesn't own them)
        drop(cache);

        let after_cache_drop = settled_alive_tasks().await;
        assert_eq!(
            after_cache_drop,
            baseline + 2,
            "Dropping cache should not affect tasks"
        );

        // Dropping the handle should cause both tasks to exit
        drop(handle);

        let after_handle_drop = await_alive_tasks(baseline).await;
        assert_eq!(
            after_handle_drop, baseline,
            "All tasks should exit after Handle is dropped"
        );
    }
}

/// Two actors holding handles to each other, which is the shape that deadlocks.
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct Parser {
    store: Option<Handle<Store>>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct Store {
    parser: Option<Handle<Parser>>,
}

#[actify]
impl Parser {
    async fn parse(&self) {
        self.store.as_ref().unwrap().save().await;
    }

    async fn is_ready(&self) -> bool {
        true
    }
}

#[actify]
impl Store {
    async fn save(&self) {
        self.parser.as_ref().unwrap().is_ready().await;
    }
}
