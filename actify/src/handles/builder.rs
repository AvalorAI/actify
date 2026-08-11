use std::any::type_name;
use std::fmt::{self, Debug};
use std::marker::PhantomData;

use super::handle::{BroadcastAs, Handle};

/// The number of queued jobs a [`Handle`] accepts before callers wait.
pub(crate) const DEFAULT_JOB_CAPACITY: usize = 100;

/// The number of broadcast values a subscriber may fall behind by before it lags.
pub(crate) const DEFAULT_BROADCAST_CAPACITY: usize = 100;

/// Configures the channel capacities of a [`Handle`] before spawning its actor.
///
/// Obtained via [`Handle::builder`]. Both capacities default to 100.
///
/// # Examples
///
/// ```
/// # use actify::Handle;
/// # #[tokio::main]
/// # async fn main() {
/// let handle: Handle<i32> = Handle::builder(1)
///     .job_capacity(8)
///     .broadcast_capacity(1024)
///     .spawn();
///
/// assert_eq!(handle.get().await, 1);
/// # }
/// ```
pub struct HandleBuilder<T, V = T> {
    value: T,
    job_capacity: usize,
    broadcast_capacity: usize,
    broadcast_type: PhantomData<fn() -> V>,
}

impl<T, V> Debug for HandleBuilder<T, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(&format!("HandleBuilder<{}>", type_name::<T>()))
            .field("job_capacity", &self.job_capacity)
            .field("broadcast_capacity", &self.broadcast_capacity)
            .finish()
    }
}

impl<T, V> HandleBuilder<T, V> {
    pub(super) fn new(value: T) -> Self {
        HandleBuilder {
            value,
            job_capacity: DEFAULT_JOB_CAPACITY,
            broadcast_capacity: DEFAULT_BROADCAST_CAPACITY,
            broadcast_type: PhantomData,
        }
    }

    /// Sets how many jobs may queue before callers wait for a slot.
    ///
    /// A job is one method call on the handle. The actor runs one at a time, so
    /// this bounds how far callers may run ahead of it before
    /// [`Handle::capacity`] reaches zero and the next caller waits.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    pub fn job_capacity(mut self, capacity: usize) -> Self {
        assert!(capacity > 0, "job_capacity must be greater than zero");
        self.job_capacity = capacity;
        self
    }

    /// Sets how many broadcast values a subscriber may fall behind by.
    ///
    /// A subscriber that falls further behind loses the oldest values and is
    /// told how many by [`RecvError::Lagged`](tokio::sync::broadcast::error::RecvError::Lagged),
    /// or by [`CacheRecvError::Lagged`](crate::CacheRecvError::Lagged) when it reads through a
    /// [`Cache`](crate::Cache). Tokio rounds this up to a power of two.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    pub fn broadcast_capacity(mut self, capacity: usize) -> Self {
        assert!(capacity > 0, "broadcast_capacity must be greater than zero");
        self.broadcast_capacity = capacity;
        self
    }
}

impl<T, V> HandleBuilder<T, V>
where
    T: BroadcastAs<V> + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Spawns the actor and returns its [`Handle`].
    pub fn spawn(self) -> Handle<T, V> {
        Handle::spawn_with(self.value, self.job_capacity, self.broadcast_capacity)
    }
}

#[cfg(test)]
mod tests {
    use crate::Handle;
    use tokio::sync::broadcast::error::RecvError;

    const SENT: i32 = 3;

    #[tokio::test(start_paused = true)]
    async fn test_the_job_capacity_is_configurable() {
        let handle: Handle<i32> = Handle::builder(0).job_capacity(5).spawn();

        assert_eq!(handle.capacity(), 5);
        assert_ne!(Handle::new(0).capacity(), 5, "5 is the default capacity");
    }

    #[tokio::test(start_paused = true)]
    async fn test_the_broadcast_capacity_is_configurable() {
        let handle: Handle<i32> = Handle::builder(0).broadcast_capacity(2).spawn();
        let mut rx = handle.subscribe();

        for value in 1..=SENT {
            handle.set(value).await;
        }

        assert_eq!(rx.recv().await, Err(RecvError::Lagged(1)));
    }

    #[tokio::test(start_paused = true)]
    async fn test_the_defaults_match_new() {
        let handle: Handle<i32> = Handle::builder(0).spawn();
        let mut rx = handle.subscribe();

        assert_eq!(handle.capacity(), Handle::new(0).capacity());

        for value in 1..=SENT {
            handle.set(value).await;
        }

        assert_eq!(
            rx.recv().await,
            Ok(1),
            "the default broadcast capacity lags"
        );
    }

    #[tokio::test(start_paused = true)]
    #[should_panic(expected = "job_capacity must be greater than zero")]
    async fn test_a_zero_job_capacity_panics() {
        let _ = Handle::<i32>::builder(0).job_capacity(0);
    }

    #[tokio::test(start_paused = true)]
    #[should_panic(expected = "broadcast_capacity must be greater than zero")]
    async fn test_a_zero_broadcast_capacity_panics() {
        let _ = Handle::<i32>::builder(0).broadcast_capacity(0);
    }
}
