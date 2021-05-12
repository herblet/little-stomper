use std::cmp::Eq;
use std::future::{ready, Future};
use std::hash::Hash;
use std::pin::Pin;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;

use std::task::{Context, Poll, Waker};

use im::HashMap;

use tokio::sync::mpsc::{self, UnboundedSender};

enum MapAction<K, V>
where
    K: Clone + Hash + Eq + Sync + Send + Unpin,
    V: Clone + Sync + Send + Unpin,
{
    GetOrCreate(K, Box<dyn Fn(&K) -> V + Send>, Waker),
    Quit,
}

struct MapReturnFuture<'a, K, V>
where
    K: Clone + Hash + Eq + Sync + Send + Unpin,
    V: Clone + Sync + Send + Unpin,
{
    update_sender: UnboundedSender<MapAction<K, V>>,
    map_ptr: Arc<AtomicPtr<HashMap<K, V>>>,
    key: &'a K,
    factory: Option<Box<dyn Fn(&K) -> V + Send>>,
}

impl<'a, K, V> Future for MapReturnFuture<'a, K, V>
where
    K: 'static + Clone + Hash + Eq + Sync + Send + Unpin,
    V: Clone + Sync + Send + Unpin + 'static,
{
    type Output = V;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut mutable = self;
        match NonLockingMap::get_sync(&mutable.map_ptr, &mutable.key) {
            Some(value) => Poll::Ready(value),
            None => {
                let factory = mutable.factory.take().unwrap();

                match mutable.update_sender.send(MapAction::GetOrCreate(
                    mutable.key.clone(),
                    factory,
                    cx.waker().clone(),
                )) {
                    Ok(_) => Poll::Pending,
                    Err(_) => Poll::Pending,
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct NonLockingMap<K, V>
where
    K: Clone + Hash + Eq + Sync + Send + Unpin,
    V: Clone + Sync + Send + Unpin,
{
    update_sender: UnboundedSender<MapAction<K, V>>,
    map_ptr: Arc<AtomicPtr<HashMap<K, V>>>,
    //update_task_handle: Option<JoinHandle<()>>
}

impl<K, V> NonLockingMap<K, V>
where
    K: 'static + Clone + Hash + Eq + Sync + Send + Unpin,
    V: 'static + Clone + Sync + Send + Unpin,
{
    fn get_sync<'a>(map_ptr: &Arc<AtomicPtr<HashMap<K, V>>>, key: &'a K) -> Option<V> {
        unsafe {
            (*map_ptr.load(Ordering::Relaxed))
                .get(key)
                .map(|val| val.clone())
        }
    }

    pub fn new() -> Self {
        let (update_sender, mut update_receiver) = mpsc::unbounded_channel();

        let mut hashmap = Box::pin(HashMap::default());
        let map_ptr = Arc::new(AtomicPtr::new(&mut *hashmap));

        let non_locking_map = NonLockingMap {
            update_sender,
            map_ptr,
            //update_task_handle: None
        };

        let mut cloned = non_locking_map.clone();

        Some(tokio::task::spawn(async move {
            let mut moved_map = hashmap; // The hashmap ist moved to the task, so it will live as long as the task keeps it

            while let Some(action) = update_receiver.recv().await {
                match action {
                    MapAction::Quit => break,
                    MapAction::GetOrCreate(key, factory, waker) => {
                        if let Some(new_map) =
                            cloned.create_if_necessary(&mut moved_map, key, factory)
                        {
                            moved_map = new_map;
                        }
                        waker.wake(); // Let the calling thread know that the map has been updated
                    }
                }
            }
        }));

        non_locking_map
    }

    fn create_if_necessary(
        &mut self,
        map: &mut Pin<Box<HashMap<K, V>>>,
        key: K,
        factory: Box<dyn Fn(&K) -> V + Send>,
    ) -> Option<Pin<Box<HashMap<K, V>>>> {
        match self.get_sync_instance(&key) {
            Some(value) => {
                // nothing to do; probably multiple creates were queued up for the same key
                None
            }
            None => {
                let value = factory(&key);
                let mut cloned_map = Box::pin((*map).update(key, value));
                let map_ptr: *mut HashMap<K, V> = &mut *cloned_map;
                self.map_ptr.swap(map_ptr, Ordering::Relaxed);

                Some(cloned_map)
            }
        }
    }

    fn get_sync_instance(&self, key: &K) -> Option<V> {
        NonLockingMap::get_sync(&self.map_ptr, key)
    }

    /// Synchronously returns the value associated with the provided key, if present; otherwise None
    pub fn get_if_present(&self, key: &K) -> Option<V> {
        self.get_sync_instance(key)
    }

    pub fn get<'a, 'b>(
        &self,
        key: &'a K,
        factory: Box<dyn Fn(&K) -> V + Send + 'static>,
    ) -> impl Future<Output = V> + 'a {
        MapReturnFuture {
            key,
            factory: Some(factory),
            map_ptr: self.map_ptr.clone(),
            update_sender: self.update_sender.clone(),
        }
    }
}

#[cfg(test)]
mod test {

    use super::NonLockingMap;

    #[tokio::test]
    async fn get_sync() {
        let map = NonLockingMap::<String, String>::new();

        assert_eq!(None, map.get_if_present(&"foo".to_owned()));
    }

    #[tokio::test]
    async fn get_sync2() {
        let map = NonLockingMap::<String, String>::new();

        let key = "foo".to_owned();

        let future = map.get(&key, Box::new(|key| format!("Hello, {}!", key)));

        assert_eq!(None, map.get_if_present(&key));
        let value = future.await;

        assert_eq!("Hello, foo!", value);
        assert_eq!("Hello, foo!", map.get_if_present(&key).unwrap());
    }
}
