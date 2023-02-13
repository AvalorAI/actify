use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::hash::Hash;
use std::{any::Any, fmt::Debug};

use crate::{
    actors::{ActorError, WRONG_ARGS, WRONG_RESPONSE},
    FnType, Handle,
};

use super::any::Actor;

#[async_trait]
pub trait MapHandle<K, V>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    V: Clone + Debug + Send + Sync + 'static,
{
    async fn get_key(&self, key: K) -> Result<Option<V>, ActorError>;

    async fn insert(&self, key: K, val: V) -> Result<Option<V>, ActorError>;

    async fn is_empty(&self) -> Result<bool, ActorError>;
}

#[async_trait]
impl<K, V> MapHandle<K, V> for Handle<HashMap<K, V>>
where
    K: Clone + Debug + Eq + Hash + Send + Sync + 'static,
    V: Clone + Debug + Send + Sync + 'static,
{
    async fn get_key(&self, key: K) -> Result<Option<V>, ActorError> {
        let res = self.send_job(FnType::Inner(Box::new(MapActor::get_key)), Box::new(key)).await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    async fn insert(&self, key: K, val: V) -> Result<Option<V>, ActorError> {
        let res = self
            .send_job(FnType::Inner(Box::new(MapActor::insert)), Box::new((key, val)))
            .await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }

    async fn is_empty(&self) -> Result<bool, ActorError> {
        let res = self.send_job(FnType::Inner(Box::new(MapActor::is_empty)), Box::new(())).await?;
        Ok(*res.downcast().expect(WRONG_RESPONSE))
    }
}

trait MapActor {
    fn get_key(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>;

    fn insert(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>;

    fn is_empty(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>;
}

impl<K, V> MapActor for Actor<HashMap<K, V>>
where
    K: Clone + Debug + Eq + Hash + Send + 'static,
    V: Clone + Debug + Send + 'static,
{
    fn get_key(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        let val = *args.downcast().expect(WRONG_ARGS);
        let res = self
            .inner
            .as_mut()
            .ok_or_else(|| ActorError::NoValueSet(std::any::type_name::<HashMap<K, V>>().to_string()))?
            .get(&val)
            .cloned();
        Ok(Box::new(res))
    }

    fn insert(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        let (key, val) = *args.downcast().expect(WRONG_ARGS);
        let res = self
            .inner
            .as_mut()
            .ok_or_else(|| ActorError::NoValueSet(std::any::type_name::<HashMap<K, V>>().to_string()))?
            .insert(key, val);
        self.broadcast();
        Ok(Box::new(res))
    }

    fn is_empty(&mut self, _args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        if let Some(inner) = &self.inner {
            if inner.is_empty() {
                Ok(Box::new(true)) // If map set, but still empty
            } else {
                Ok(Box::new(false)) // Any set and filled map
            }
        } else {
            Ok(Box::new(true)) // If none set, its empty
        }
    }
}
