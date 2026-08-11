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

        assert_eq!(rx.recv().await, Ok(1), "the default broadcast capacity lags");
    }
}
