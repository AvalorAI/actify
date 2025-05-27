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
    #[cfg(target_os = "linux")]
    fn foo(&mut self, i: i32, _h: HashMap<String, T>) -> f64 {
        (i + 1) as f64
    }

    #[cfg(target_os = "windows")]
    fn foo(&mut self, i: i32, _h: HashMap<String, T>) -> f64 {
        (i + 2) as f64
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

#[cfg(test)]
mod tests {
    use super::*;
    use actify::{Handle, VecHandle};
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_macro() {
        let actor_handle = Handle::new(TestStruct {
            inner_data: "Test".to_string(),
        });

        #[cfg(target_os = "linux")]
        assert_eq!(actor_handle.foo(0, HashMap::new()).await, 1.);

        #[cfg(target_os = "windows")]
        assert_eq!(actor_handle.foo(0, HashMap::new()).await, 2.);

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
