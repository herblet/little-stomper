use crate::client::DefaultClient;
use crate::destinations::*;
use crate::error::StomperError;

use async_map::{AsyncFactory, AsyncMap, VersionedMap};

use std::ops::Deref;
use std::sync::Arc;

use futures::FutureExt;

pub trait DestinationType:
    Destination<Client = DefaultClient> + Send + Unpin + Sync + Clone + std::fmt::Debug + 'static
{
}

impl<
        T: Destination<Client = DefaultClient>
            + Send
            + Unpin
            + Sync
            + Clone
            + std::fmt::Debug
            + 'static,
    > DestinationType for T
{
}

#[derive(Clone)]
pub struct AsyncDestinations<D: DestinationType> {
    destinations: VersionedMap<DestinationId, D>,
    destination_factory: Arc<dyn AsyncFactory<DestinationId, D>>,
}

impl<D: DestinationType> Destinations for AsyncDestinations<D> {
    type Client = D::Client;

    fn subscribe<S: Subscriber + 'static, E: Deref<Target = S> + Clone + Send + 'static>(
        &self,
        destination: DestinationId,
        subscriber_sub_id: Option<SubscriptionId>,
        subscriber: E,
        client: &Self::Client,
    ) {
        let client = client.clone();
        tokio::task::spawn(
            self.destinations
                .get(&destination, self.destination_factory.clone())
                .inspect(move |destination| {
                    destination.subscribe(subscriber_sub_id, subscriber, &client);
                }),
        );
    }

    fn send<S: Sender + 'static, E: Deref<Target = S> + Clone + Send + 'static>(
        &self,
        destination: DestinationId,
        message: InboundMessage,
        sender: E,
        client: &Self::Client,
    ) {
        // Clone it, so that potential updates don't affect this copy
        // let destinations = self.cloned_dest();
        if let Some(destination) = self.destinations.get_if_present(&destination) {
            destination.send(message, sender, client);
        } else {
            sender.send_callback(
                message.sender_message_id,
                Err(StomperError::new(
                    format!("Unknown destination '{}'", destination).as_str(),
                )),
            );
        }
    }
    fn unsubscribe<S: Subscriber + 'static, E: Deref<Target = S> + Clone + Send + 'static>(
        &self,
        destination: DestinationId,
        subscription: SubscriptionId,
        subscriber: E,
        client: &Self::Client,
    ) {
        if let Some(destination) = self.destinations.get_if_present(&destination) {
            destination.unsubscribe(subscription, subscriber, client);
        } else {
            log::info!("Requested unsubscribe for unknown destination");
        }
    }
}

impl<D: DestinationType> AsyncDestinations<D> {
    pub async fn start(
        destination_factory: Arc<dyn AsyncFactory<DestinationId, D>>,
    ) -> AsyncDestinations<D> {
        AsyncDestinations {
            destinations: VersionedMap::new(),
            destination_factory,
        }
    }
}

#[cfg(test)]
mod test {

    use super::super::mocks::*;
    use super::AsyncDestinations;
    use crate::client::DefaultClient;
    use crate::destinations::*;

    use std::ops::Deref;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, RwLock};

    use async_map::AsyncFactory;
    use tokio::task::yield_now;

    use im::HashMap;

    // Turn an Arc<Destination> into a Destination
    impl<A: Destination + std::marker::Sync> Destination for Arc<A> {
        type Client = A::Client;

        fn subscribe<S: Subscriber + 'static, E: Deref<Target = S> + Clone + Send + 'static>(
            &self,
            subscriber_sub_id: Option<SubscriptionId>,
            subscriber: E,
            client: &Self::Client,
        ) {
            self.as_ref()
                .subscribe(subscriber_sub_id, subscriber, client)
        }
        fn unsubscribe<S: Subscriber + 'static, E: Deref<Target = S> + Clone + Send + 'static>(
            &self,
            sub: SubscriptionId,
            subscriber: E,
            client: &Self::Client,
        ) {
            self.as_ref().unsubscribe(sub, subscriber, client)
        }
        fn send<S: Sender + 'static, E: Deref<Target = S> + Clone + Send + 'static>(
            &self,
            message: InboundMessage,
            sender: E,
            client: &Self::Client,
        ) {
            self.as_ref().send(message, sender, client)
        }
        fn close(&self) {
            self.as_ref().close()
        }
    }

    fn map_draining_factory(
        map: Arc<RwLock<HashMap<DestinationId, Arc<MockTestDest>>>>,
    ) -> Arc<dyn AsyncFactory<DestinationId, Arc<MockTestDest>>> {
        Arc::new(move |id| map.try_write().unwrap().remove(id).unwrap())
    }

    fn insert_subscribe_counting_dest(
        id: DestinationId,
        expected_sub_id: SubscriptionId,
        counter: Arc<AtomicUsize>,
        map: &mut Arc<RwLock<HashMap<DestinationId, Arc<MockTestDest>>>>,
    ) {
        let mut dest = MockTestDest::new();
        dest.expect_subscribe::<MockTestSubscriber, Arc<MockTestSubscriber>>()
            .withf(move |subscriber_sub_id, _, _| {
                counter.fetch_add(1, Ordering::Release);
                subscriber_sub_id
                    .as_ref()
                    .map(|subscriber_sub_id| *subscriber_sub_id == expected_sub_id)
                    .unwrap_or(false)
            })
            .return_const(());
        let dest = Arc::new(dest);
        map.try_write().unwrap().insert(id, dest);
    }

    fn insert_send_counting_dest(
        id: DestinationId,
        expected_message_id: MessageId,
        counter: Arc<AtomicUsize>,
        map: &mut Arc<RwLock<HashMap<DestinationId, Arc<MockTestDest>>>>,
    ) -> Arc<MockTestDest> {
        let mut dest = MockTestDest::new();
        dest.expect_subscribe::<MockTestSubscriber, &MockTestSubscriber>()
            .return_const(());

        dest.expect_send::<MockTestSender, Arc<MockTestSender>>()
            .withf(move |message, _, _| {
                counter.fetch_add(1, Ordering::Release);
                message
                    .sender_message_id
                    .as_ref()
                    .map(|sender_message_id| *sender_message_id == expected_message_id)
                    .unwrap_or(false)
            })
            .return_const(());
        let dest = Arc::new(dest);
        map.try_write().unwrap().insert(id, dest.clone());
        dest
    }

    #[tokio::test]
    async fn destinations_creates_on_subscribe() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut expected_destinations =
            Arc::<RwLock<HashMap<DestinationId, Arc<MockTestDest>>>>::default();

        let foo = DestinationId::from("foo");

        let sub_id = SubscriptionId::from("bar");

        insert_subscribe_counting_dest(
            foo.clone(),
            sub_id.clone(),
            count.clone(),
            &mut expected_destinations,
        );

        let destinations = AsyncDestinations::<Arc<MockTestDest>>::start(map_draining_factory(
            expected_destinations.clone(),
        ))
        .await;

        let subscriber = create_subscriber();

        destinations.subscribe(foo, Some(sub_id), subscriber, &DefaultClient);

        yield_now().await;

        // Check all expected destinations were "created"
        assert_eq!(0, expected_destinations.try_read().unwrap().len());

        // check that subscribe was called once
        assert_eq!(1, count.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn destinations_sends_to_created_destination() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut expected_destinations =
            Arc::<RwLock<HashMap<DestinationId, Arc<MockTestDest>>>>::default();

        let foo = DestinationId::from("foo");

        let forty_two = MessageId::from("42");

        insert_send_counting_dest(
            foo.clone(),
            forty_two.clone(),
            count.clone(),
            &mut expected_destinations,
        );

        let destinations = AsyncDestinations::<Arc<MockTestDest>>::start(map_draining_factory(
            expected_destinations.clone(),
        ))
        .await;

        let subscriber = create_subscriber();

        destinations.subscribe(foo.clone(), None, subscriber.clone(), &DefaultClient);

        yield_now().await;

        let sender = create_sender();

        destinations.send(
            foo,
            InboundMessage {
                sender_message_id: Some(forty_two),
                body: Vec::new(),
            },
            sender,
            &DefaultClient,
        );

        // Check all expected destinations were "created"
        assert_eq!(0, expected_destinations.try_read().unwrap().len());

        // check that subscribe was called once
        assert_eq!(1, count.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn destinations_creates_each_destination() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut expected_destinations =
            Arc::<RwLock<HashMap<DestinationId, Arc<MockTestDest>>>>::default();

        let foo = DestinationId(String::from("foo"));
        let sub_id_foo = SubscriptionId::from("bar");

        insert_subscribe_counting_dest(
            foo.clone(),
            sub_id_foo.clone(),
            count.clone(),
            &mut expected_destinations,
        );

        let bar = DestinationId(String::from("bar"));
        let sub_id_bar = SubscriptionId::from("foo");

        insert_subscribe_counting_dest(
            bar.clone(),
            sub_id_bar.clone(),
            count.clone(),
            &mut expected_destinations,
        );
        let destinations = AsyncDestinations::<Arc<MockTestDest>>::start(map_draining_factory(
            expected_destinations.clone(),
        ))
        .await;

        destinations.subscribe(foo, Some(sub_id_foo), create_subscriber(), &DefaultClient);

        yield_now().await;

        destinations.subscribe(bar, Some(sub_id_bar), create_subscriber(), &DefaultClient);

        yield_now().await;

        // Check all expected destinations were "created"
        assert_eq!(0, expected_destinations.try_read().unwrap().len());
        // check that sub was called twice
        assert_eq!(2, count.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn destinations_reuses_on_subsequent_subscribe() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut expected_destinations =
            Arc::<RwLock<HashMap<DestinationId, Arc<MockTestDest>>>>::default();

        let foo = DestinationId(String::from("foo"));
        let sub_id_foo = SubscriptionId::from("bar");

        insert_subscribe_counting_dest(
            foo.clone(),
            sub_id_foo.clone(),
            count.clone(),
            &mut expected_destinations,
        );

        let destinations = AsyncDestinations::<Arc<MockTestDest>>::start(map_draining_factory(
            expected_destinations.clone(),
        ))
        .await;

        destinations.subscribe(
            foo.clone(),
            Some(sub_id_foo.clone()),
            create_subscriber(),
            &DefaultClient,
        );

        destinations.subscribe(
            foo.clone(),
            Some(sub_id_foo.clone()),
            create_subscriber(),
            &DefaultClient,
        );

        destinations.subscribe(
            foo.clone(),
            Some(sub_id_foo.clone()),
            create_subscriber(),
            &DefaultClient,
        );

        destinations.subscribe(foo, Some(sub_id_foo), create_subscriber(), &DefaultClient);

        yield_now().await;

        // Check all expected destinations were "created"
        assert_eq!(0, expected_destinations.try_read().unwrap().len());

        // check that subscribe was called four times
        assert_eq!(4, count.load(Ordering::Relaxed));
    }
}
