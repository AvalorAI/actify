use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::oneshot::error::RecvError;
use tonic::Status;

pub mod base;
pub mod cache;
pub mod map;
pub mod vec;

pub use base::{Container, FnType, Handle}; // Reexport for easier reference
pub use map::MapHandle; // Reexport for easier reference
pub use vec::VecHandle; // Reexport for easier reference

const CHANNEL_SIZE: usize = 100;
const WRONG_ARGS: &str = "Incorrect arguments have been provided for this method";
const WRONG_RESPONSE: &str = "An incorrect response type for this method has been called";

#[derive(Error, Debug, PartialEq, Clone)]
pub enum ActorError {
    #[error("A request has been received for type {0} while no value is set")]
    NoValueSet(String),
    #[error("The response of a request has timed out")]
    TimeOut, // TODO not yet implemented
    #[error("Error from actor function evaluation: {0}")]
    EvalError(String), // Originates from general eval format
    #[error("Tokio receiver error")]
    TokioRecvError(#[from] RecvError),
    #[error("Tokio sender error")]
    TokioSendError,
    #[error("An incorrect response type for this method has been received")]
    WrongResponse,
}

// Convert any actor error to an internal failure
impl From<ActorError> for Status {
    fn from(_e: ActorError) -> Self {
        Status::internal("Actor model failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, Result};
    use std::any::Any;
    use std::collections::HashMap;

    #[tokio::test]
    async fn receive_val_broadcast() {
        let handle = Handle::new();
        let mut rx = handle.subscribe().await.unwrap();
        handle.set(Some("testing!")).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), Some("testing!"));
    }

    #[tokio::test]
    async fn set_ok_actor() {
        let handle = Handle::new();
        handle.set(1).await.unwrap();
    }

    #[tokio::test]
    async fn get_ok_actor() {
        let handle = Handle::new();
        handle.set(1).await.unwrap();
        let result = handle.get().await;
        assert_eq!(result.unwrap(), 1);
    }

    #[tokio::test]
    async fn get_err_actor() {
        let handle = Handle::<i32>::new();
        assert!(handle.get().await.is_err());
    }

    #[tokio::test]
    async fn no_val_set_actor() {
        let handle = Handle::<Vec<i32>>::new();
        let err = handle.push(10).await.unwrap_err();
        assert!(matches!(err, ActorError::NoValueSet(_)))
    }

    #[tokio::test]
    async fn push_to_actor() {
        let handle = Handle::new();
        handle.set(vec![1, 2]).await.unwrap();
        handle.push(100).await.unwrap();
        assert_eq!(handle.get().await.unwrap(), vec![1, 2, 100]);
    }

    #[tokio::test]
    async fn drain_actor() {
        let handle = Handle::new();
        handle.set(vec![1, 2]).await.unwrap();
        let res = handle.drain().await.unwrap();
        assert_eq!(res, vec![1, 2]);
        assert_eq!(handle.get().await.unwrap(), Vec::<i32>::new());
    }

    #[tokio::test]
    async fn insert_at_actor() {
        let handle = Handle::new();
        handle.set(HashMap::new()).await.unwrap();
        let res = handle.insert("test", 10).await.unwrap();
        assert_eq!(res, None);
    }

    #[tokio::test]
    async fn insert_overwrite_at_actor() {
        let handle = Handle::new();
        handle.set(HashMap::new()).await.unwrap();
        handle.insert("test", 10).await.unwrap();
        let old_value = handle.insert("test", 20).await.unwrap();
        assert_eq!(old_value, Some(10));
    }

    #[tokio::test]
    async fn get_key_actor() {
        let handle = Handle::new();
        handle.set(HashMap::new()).await.unwrap();
        handle.insert("test", 10).await.unwrap();
        let res = handle.get_key("test").await.unwrap();
        assert_eq!(res, Some(10));
    }

    #[tokio::test]
    async fn get_empty_from_actor() {
        let handle = Handle::new();
        handle.set(HashMap::<&str, i32>::new()).await.unwrap();
        let res = handle.get_key("test").await.unwrap();
        assert_eq!(res, None);
    }

    #[tokio::test]
    async fn actor_is_none() {
        let handle = Handle::<i32>::new();
        assert_eq!(handle.is_none().await.unwrap(), true);
    }

    #[tokio::test]
    async fn actor_is_not_none() {
        let handle = Handle::new();
        handle.set(1).await.unwrap();
        assert_eq!(handle.is_none().await.unwrap(), false);
    }

    #[tokio::test]
    async fn actor_hashmap_is_empty() {
        let handle = Handle::new_from(HashMap::<&str, i32>::new());
        assert_eq!(handle.is_empty().await.unwrap(), true);
    }

    #[tokio::test]
    async fn actor_hashmap_is_not_empty() {
        let handle = Handle::new_from(HashMap::new());
        handle.insert("test", 1).await.unwrap();
        assert_eq!(handle.is_none().await.unwrap(), false);
    }

    #[tokio::test]
    async fn actor_vec_is_empty() {
        let handle = Handle::new_from(Vec::<i32>::new());
        assert_eq!(handle.is_empty().await.unwrap(), true);
    }

    #[tokio::test]
    async fn actor_vec_is_not_empty() {
        let handle = Handle::new_from(Vec::new());
        handle.push(1).await.unwrap();
        assert_eq!(handle.is_none().await.unwrap(), false);
    }

    #[tokio::test]
    async fn eval_empty_actor() {
        let handle = Handle::new();
        let err = handle
            .eval::<_, _, i32>(TestVal::heavy_calcs, 10)
            .await
            .unwrap_err();
        assert!(matches!(err, ActorError::NoValueSet(_)))
    }

    #[tokio::test]
    async fn eval_ok_actor() {
        let handle = Handle::new_from(TestVal {});
        let res: i32 = handle.eval(TestVal::heavy_calcs, 10).await.unwrap();
        assert_eq!(res, 11);
    }

    #[tokio::test]
    async fn eval_cast_resp_err_actor() {
        let handle = Handle::new_from(TestVal {});
        let err = handle
            .eval::<_, _, String>(TestVal::heavy_calcs, 10)
            .await
            .unwrap_err();
        assert_eq!(ActorError::WrongResponse, err);
    }

    #[tokio::test]
    async fn eval_cast_args_err_actor() {
        let handle = Handle::new_from(TestVal {});
        let err = handle
            .eval::<_, _, i32>(TestVal::heavy_calcs, "test".to_string())
            .await
            .unwrap_err();
        assert!(matches!(err, ActorError::EvalError(_)))
    }

    #[derive(Clone, Debug)]
    struct TestVal {}

    impl TestVal {
        fn heavy_calcs(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>> {
            let val = *args
                .downcast::<i32>()
                .map_err(|_| anyhow!("Downcasting the args went wrong"))?;
            Ok(Box::new(val + 1))
        }
    }
}
