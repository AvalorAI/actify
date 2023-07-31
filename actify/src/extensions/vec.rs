use crate as actify;
use actify_macros::actify;
use std::fmt::Debug;

trait ActorVec<T> {
    fn push(&mut self, value: T);

    fn is_empty(&self) -> bool;

    fn drain_all(&mut self) -> Vec<T>;
}

#[actify]
impl<T> ActorVec<T> for Vec<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    /// Appends an element to the back of a collection.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio;
    /// # #[tokio::test]
    /// # async fn push_to_actor() {
    /// let handle = actify::Handle::new(vec![1, 2]);
    /// handle.push(100).await.unwrap();
    /// assert_eq!(handle.get().await.unwrap(), vec![1, 2, 100]);
    /// # }
    /// ```
    fn push(&mut self, value: T) {
        self.push(value)
    }

    /// Returns `true` if the vector contains no elements.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio;
    /// # use actify;
    /// # #[tokio::test]
    /// # async fn actor_vec_is_empty() {
    /// let handle = Handle::new(Vec::<i32>::new());
    /// assert!(handle.is_empty().await.unwrap());
    /// # }
    /// ```
    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    /// Removes the complete range from the vector in bulk, and returns it as a new vector
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio;
    /// # #[tokio::test]
    /// # async fn drain_actor() {
    /// let handle = Handle::new(vec![1, 2]);
    /// let res = handle.drain_all().await.unwrap();
    /// assert_eq!(res, vec![1, 2]);
    /// assert_eq!(handle.get().await.unwrap(), Vec::<i32>::new());
    /// # }
    /// ```
    fn drain_all(&mut self) -> Vec<T> {
        // TODO add actual range as with the std vec
        // TODO this is currently not possible without supporting generic method arguments
        self.drain(..).collect()
    }
}
