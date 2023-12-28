//! This workspace is used to test the functionalities of actify as would any user that imports the library

fn main() {}

#[cfg(test)]
mod tests {

    use actify::{actify, Actor, ActorError, FnType, Handle};
    use async_trait::async_trait;
    use std::{collections::HashMap, fmt::Debug, time::Duration};
    use tokio::time::sleep;

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
        #[cfg(not(feature = "test_feature"))] // TODO should be replaced by os cfg types
        fn foo(&mut self, i: i32, _h: HashMap<String, T>) -> f64 {
            (i + 1) as f64
        }

        async fn baz(&mut self, i: i32) -> f64 {
            (i + 2) as f64
        }
    }

    impl<T> TestStruct<T>
    where
        T: Clone + Debug + Send + Sync + 'static,
    {
        // TODO generics in arguments are not supported yet
        fn bar<F>(&self, i: usize, _f: F) -> f32
        where
            F: Debug + Send + Sync,
        {
            (i + 1) as f32
        }
    }

    #[async_trait]
    trait ExampleTestStructHandle<F> {
        async fn bar(&self, test: usize, f: F) -> Result<f32, ActorError>;
    }

    #[async_trait]
    impl<T, F> ExampleTestStructHandle<F> for Handle<TestStruct<T>>
    where
        T: Clone + Debug + Send + Sync + 'static,
        F: Debug + Send + Sync + 'static,
    {
        async fn bar(&self, test: usize, t: F) -> Result<f32, ActorError> {
            let res = self
                .send_job(
                    FnType::Inner(Box::new(ExampleTestStructActor::<F>::_bar)),
                    Box::new((test, t)),
                )
                .await?;
            Ok(*res.downcast().unwrap())
        }
    }

    trait ExampleTestStructActor<F> {
        fn _bar(
            &mut self,
            args: Box<dyn std::any::Any + Send>,
        ) -> Result<Box<dyn std::any::Any + Send>, ActorError>;
    }

    #[allow(unused_parens)]
    impl<T, F> ExampleTestStructActor<F> for Actor<TestStruct<T>>
    where
        T: Clone + Debug + Send + Sync + 'static,
        F: Debug + Send + Sync + 'static,
    {
        fn _bar(
            &mut self,
            args: Box<dyn std::any::Any + Send>,
        ) -> Result<Box<dyn std::any::Any + Send>, ActorError> {
            let (test, f): (usize, F) = *args.downcast().unwrap();

            let result: f32 = self.inner.bar(test, f);

            self.broadcast();

            Ok(Box::new(result))
        }
    }

    #[tokio::test]
    async fn test_macro() {
        let actor_handle = Handle::new(TestStruct {
            inner_data: "Test".to_string(),
        });

        assert_eq!(actor_handle.bar(0, String::default()).await.unwrap(), 1.);
        assert_eq!(actor_handle.foo(0, HashMap::new()).await.unwrap(), 1.);
        assert_eq!(actor_handle.baz(0).await.unwrap(), 2.);
    }

    #[tokio::test]
    async fn test_handle_out_of_scope() {
        // load_logger();

        let handle_1 = Handle::new(1);

        let mut cache_3 = {
            let _handle_2 = Handle::new("test");
            let handle_3 = Handle::new(1.); // This goes out of scope
            let _handle_1_clone = handle_1.clone();
            let cache_3 = handle_3.create_initialized_cache().await.unwrap(); // But the cache doesn't
            cache_3
        };

        sleep(Duration::from_secs(1)).await;

        // The &str actor should have exited --> watch logs
        // The f32 should have exited, even though the cache is still in scope!
        assert!(cache_3.try_recv_newest().is_err()) // This means the cache has no broadcast anymore, so it should exit too
    }

    #[allow(dead_code)]
    pub fn load_logger() {
        env_logger::Builder::new()
            .filter(None, log::LevelFilter::Info)
            .init();
    }
}
