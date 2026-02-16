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

#[cfg(test)]
mod tests {
    use super::*;
    use actify::{Frequency, Handle, Throttle, VecHandle};
    use std::sync::{Arc, Mutex};
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

    /// Helper to get current number of alive tasks in the runtime
    fn alive_tasks() -> usize {
        tokio::runtime::Handle::current().metrics().num_alive_tasks()
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
    }

    #[tokio::test]
    async fn test_handle_task_cleanup() {
        // Record baseline task count
        let baseline = alive_tasks();

        // Creating a Handle spawns a Listener task
        let handle = Handle::new(42);
        sleep(Duration::from_millis(10)).await;

        let with_handle = alive_tasks();
        assert!(
            with_handle > baseline,
            "Expected task count to increase after creating Handle. Baseline: {}, After: {}",
            baseline,
            with_handle
        );

        // Drop the handle - this should cause the Listener task to exit
        drop(handle);
        sleep(Duration::from_millis(100)).await;

        let after_drop = alive_tasks();
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
        sleep(Duration::from_millis(10)).await;

        let with_handles = alive_tasks();
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
        sleep(Duration::from_millis(50)).await;

        let after_first_drop = alive_tasks();
        assert_eq!(
            after_first_drop,
            baseline + 1,
            "Task should still be running after dropping one clone. Baseline: {}, After: {}",
            baseline,
            after_first_drop
        );

        // Dropping the last clone should cause the task to exit
        drop(handle_clone);
        sleep(Duration::from_millis(100)).await;

        let after_all_drop = alive_tasks();
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
        sleep(Duration::from_millis(10)).await;

        let with_handle = alive_tasks();
        assert_eq!(
            with_handle,
            baseline + 1,
            "Expected one task for Handle"
        );

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
        sleep(Duration::from_millis(10)).await;

        let with_throttle = alive_tasks();
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
        sleep(Duration::from_millis(200)).await;

        let after_drop = alive_tasks();
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
        sleep(Duration::from_millis(10)).await;

        let with_throttle = alive_tasks();
        assert_eq!(
            with_throttle,
            baseline + 1,
            "Expected one task for interval Throttle"
        );

        // Verify the throttle is working
        sleep(Duration::from_millis(200)).await;
        let count = *client.count.lock().unwrap();
        assert!(
            count > 1,
            "Interval throttle should have fired multiple times, count: {}",
            count
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
        sleep(Duration::from_millis(10)).await;

        let with_handles = alive_tasks();
        assert_eq!(
            with_handles,
            baseline + 3,
            "Expected three tasks for three Handles"
        );

        // Drop them one by one and verify cleanup
        drop(handle1);
        sleep(Duration::from_millis(100)).await;
        assert_eq!(alive_tasks(), baseline + 2);

        drop(handle2);
        sleep(Duration::from_millis(100)).await;
        assert_eq!(alive_tasks(), baseline + 1);

        drop(handle3);
        sleep(Duration::from_millis(100)).await;
        assert_eq!(alive_tasks(), baseline);
    }

    #[tokio::test]
    async fn test_cache_does_not_spawn_tasks() {
        let baseline = alive_tasks();

        let handle = Handle::new(42);
        sleep(Duration::from_millis(10)).await;

        let with_handle = alive_tasks();
        assert_eq!(with_handle, baseline + 1, "Expected one task for Handle");

        // Creating a cache should NOT spawn additional tasks
        let _cache = handle.create_cache().await;
        sleep(Duration::from_millis(10)).await;

        let with_cache = alive_tasks();
        assert_eq!(
            with_cache,
            baseline + 1,
            "Cache should not spawn additional tasks"
        );

        // Creating more caches still shouldn't spawn tasks
        let _cache2 = handle.create_cache().await;
        let _cache3 = handle.create_cache_from_default();
        sleep(Duration::from_millis(10)).await;

        let with_more_caches = alive_tasks();
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
        sleep(Duration::from_millis(10)).await;

        let with_handle = alive_tasks();
        assert_eq!(with_handle, baseline + 1, "Expected one task for Handle");

        // Spawning a throttle from cache spawns a new task
        let client = TestClient::new();
        cache.spawn_throttle(client.clone(), TestClient::call, Frequency::OnEvent);
        sleep(Duration::from_millis(10)).await;

        let with_throttle = alive_tasks();
        assert_eq!(
            with_throttle,
            baseline + 2,
            "Expected two tasks: Handle + Throttle"
        );

        // Dropping the cache doesn't affect tasks (it doesn't own them)
        drop(cache);
        sleep(Duration::from_millis(100)).await;

        let after_cache_drop = alive_tasks();
        assert_eq!(
            after_cache_drop,
            baseline + 2,
            "Dropping cache should not affect tasks"
        );

        // Dropping the handle should cause both tasks to exit
        drop(handle);
        sleep(Duration::from_millis(200)).await;

        let after_handle_drop = alive_tasks();
        assert_eq!(
            after_handle_drop, baseline,
            "All tasks should exit after Handle is dropped"
        );
    }
}
