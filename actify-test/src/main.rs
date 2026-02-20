//! This workspace is used to test the functionalities of actify as would any user that imports the library

use actify::actify;
use std::{collections::HashMap, fmt::Debug};

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

#[cfg(test)]
mod tests {
    use super::*;
    use actify::{Handle, VecHandle};
    use std::time::Duration;
    use tokio::time::sleep;

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

    #[tokio::test]
    async fn test_handle_out_of_scope() {
        // load_logger();

        let handle_1 = Handle::new(1);

        let mut cache_3 = {
            let _handle_2 = Handle::new("test");
            let handle_3 = Handle::new(1.); // This goes out of scope
            let _handle_1_clone = handle_1.clone();
            let cache_3 = handle_3.create_cache().await; // But the cache doesn't
            cache_3
        };

        sleep(Duration::from_secs(1)).await;

        // The &str actor should have exited --> watch logs
        // The f32 should have exited, even though the cache is still in scope!
        assert!(cache_3.try_recv_newest().is_err()) // This means the cache has no broadcast anymore, so it should exit too
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

    #[allow(dead_code)]
    pub fn load_logger() {
        env_logger::Builder::new()
            .filter(None, log::LevelFilter::Info)
            .init();
    }
}
