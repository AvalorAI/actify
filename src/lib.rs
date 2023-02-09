//! Tries to compile a function including a referenced argument
//!
//! ```compile_fail
//! # struct MyActor {}
//! #[actor_model::actify]
//! impl MyActor {
//!     fn foo(&self, forbidden_reference: &usize) {
//!         println!("Hello foo: {}", forbidden_reference);
//!     }
//! }
//! ```

#![allow(rustdoc::private_intra_doc_links)]
mod actors;
mod cache;
mod throttle;

use std::any::Any;
use std::collections::HashMap;

// Reexport for easier reference
pub use actify_macros::actify;
pub use actors::any::{Actor, Handle};
pub use actors::map::MapHandle;
pub use actors::vec::VecHandle;
pub use actors::{ActorError, FnType};
pub use async_trait::async_trait;
pub use cache::Cache;
pub use throttle::{Frequency, ThrottleBuilder, Throttled};

use crate as actor_model;

#[allow(dead_code)]
#[derive(Clone)]
struct MyStruct<T> {
    inner_data: T,
}

#[actify]
impl<T> crate::MyStruct<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn foo(&mut self, i: i32, f: HashMap<String, f32>) -> f64 {
        println!("Hello foo: {}, {:?}", i, f);
        (i + 1) as f64
    }
}

impl<T> MyStruct<T> {
    fn bar(&self, i: usize) -> f32 {
        println!("Hello bar: {}", i);
        (i + 1) as f32
    }
}

// EXAMPLE TYPED OUT IMPLEMENTATION OF THE MACRO

#[async_trait]
pub trait MyActorTypedHandle {
    async fn bar(&self, test: usize) -> Result<f32, ActorError>;
}

#[async_trait]
impl<T> MyActorTypedHandle for Handle<MyStruct<T>>
where
    MyStruct<T>: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    async fn bar(&self, test: usize) -> Result<f32, ActorError> {
        let res = self
            .send_job(
                FnType::Inner(Box::new(MyStructTypedActor::_bar)),
                Box::new(test),
            )
            .await?;
        Ok(*res
            .downcast()
            .expect("Downcasting failed due to an error in the Actify macro"))
    }
}

trait MyStructTypedActor {
    fn _bar(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>;
}

#[allow(unused_parens)]
impl<T> MyStructTypedActor for Actor<MyStruct<T>>
where
    T: Clone + Send + Sync + 'static,
{
    fn _bar(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
        let (test): (usize) = *args
            .downcast()
            .expect("Downcasting failed due to an error in the Actify macro");

        let result: f32 = self
            .inner
            .as_mut()
            .ok_or(ActorError::NoValueSet(
                std::any::type_name::<MyStruct<T>>().to_string(),
            ))?
            .bar(test);

        self.broadcast();

        Ok(Box::new(result))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn test_macro() {
        let actor_handle = Handle::new_from(MyStruct {
            inner_data: "Test".to_string(),
        });

        assert_eq!(actor_handle.bar(0).await.unwrap(), 1.);
        assert_eq!(actor_handle.foo(0, HashMap::new()).await.unwrap(), 1.)
    }
}
