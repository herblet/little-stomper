use crate::client::Client;
use crate::destinations::*;
use crate::error::StomperError;

use nonlocking::{AsyncMap, FactoryBorrow, VersionedMap};

use stomp_parser::model::*;

use std::borrow::Borrow;
use std::pin::Pin;
use std::sync::Arc;

use futures::FutureExt;
use std::future::{ready, Future};

use std::collections::HashMap;
use tokio::sync::mpsc::{self, UnboundedSender};

use log;
use uuid::Uuid;

struct Subscription {
    id: SubscriptionId,
    subscriber: Box<dyn BorrowedSubscriber>,
}

impl Subscription {
    pub fn send(&mut self, message: OutboundMessage) -> Result<(), StomperError> {
        (&*self.subscriber).borrow().send(self.id.clone(), message)
        // let (content_length, body) = match &message.source_frame.body() {
        //     None => (0u32, "".to_owned()),
        //     Some(bytes) => (
        //         bytes.len() as u32,
        //         std::str::from_utf8(&bytes).unwrap().to_owned(),
        //     ),
        // };
        // if let Some(passed_length) = &message.source_frame.content_length {
        //     if *passed_length.value() != content_length as u32 {
        //         log::info!(
        //             "Strange: passed content-length {} not equal to actual length of content {}.",
        //             passed_length,
        //             content_length
        //         )
        //     }
        // }

        // let raw_body = body.as_bytes().to_owned();

        // let mut message = MessageFrame::new(
        //     MessageIdValue::new(message.message_id.clone()),
        //     DestinationValue::new(message.source_frame.destination.value().clone()),
        //     SubscriptionValue::new(self.id.clone()),
        //     Some(message.source_frame.content_type.as_ref().map_or_else(
        //         || ContentTypeValue::new("text/plain".to_owned()),
        //         |value| value.clone(),
        //     )),
        //     Some(ContentLengthValue::new(content_length)),
        //     (0, raw_body.len()),
        // );

        // message.set_raw(raw_body);

        // self.client.send(ServerFrame::Message(message))
    }
}

struct DestinationMessage {
    message_id: String,
    source_frame: SendFrame,
}

/// An action that a destination can perform
enum DestinationAction {
    Subscribe(Box<dyn BorrowedSubscriber>),
    Unsubscribe(SubscriptionId, Box<dyn BorrowedSubscriber>),
    Send(InboundMessage, Box<dyn BorrowedSender>),
    Close,
}

/// A destination that simply stores its subscriptions in memory
#[derive(Clone)]
pub struct InMemDestination {
    id: DestinationId,
    sender: tokio::sync::mpsc::UnboundedSender<DestinationAction>,
}

impl InMemDestination {
    fn perform_action(
        &self,
        action: DestinationAction,
    ) -> Result<(), mpsc::error::SendError<DestinationAction>> {
        self.sender.send(action)
    }
}

impl Destination for InMemDestination {
    fn subscribe<T: BorrowedSubscriber>(&self, subscriber: T) {
        match self.perform_action(DestinationAction::Subscribe(Box::new(subscriber))) {
            Err(err) => {
                if let mpsc::error::SendError(DestinationAction::Subscribe(subscriber)) = err {
                    (&*subscriber).borrow().subscribe_callback(
                        self.id.clone(),
                        Err(StomperError::new("Subscribe failed")),
                    );
                }
            }
            Ok(_) => { /* do nothing */ }
        }
    }

    fn send<T: BorrowedSender>(&self, message: InboundMessage, sender: T) {
        match self.perform_action(DestinationAction::Send(message, Box::new(sender))) {
            Err(err) => {
                if let mpsc::error::SendError(DestinationAction::Send(message, sender)) = err {
                    (&*sender).borrow().send_callback(
                        message.sender_message_id,
                        Err(StomperError::new("Send failed")),
                    );
                }
            }
            Ok(_) => { /* do nothing */ }
        }
    }

    fn unsubscribe<T: BorrowedSubscriber>(&self, sub_id: SubscriptionId, subscriber: T) {
        match self.perform_action(DestinationAction::Unsubscribe(sub_id, Box::new(subscriber))) {
            Err(err) => {
                if let mpsc::error::SendError(DestinationAction::Unsubscribe(sub_id, subscriber)) =
                    err
                {
                    (&*subscriber)
                        .borrow()
                        .unsubscribe_callback(sub_id, Err(StomperError::new("Unsubscribe failed")));
                }
            }
            Ok(_) => { /* do nothing */ }
        }
    }

    fn close(&self) {
        match self.perform_action(DestinationAction::Close) {
            Err(err) => {
                log::error!("Error closing destination {}: {}", self.id, err);
            }
            Ok(_) => { /* do nothing */ }
        }
    }
}

struct InMemDestinationBackend {
    id: DestinationId,
    subscriptions: HashMap<SubscriptionId, Subscription>,
}

impl InMemDestinationBackend {
    fn start(id: DestinationId, receiver: mpsc::UnboundedReceiver<DestinationAction>) {
        let backend = InMemDestinationBackend {
            id,
            subscriptions: HashMap::new(),
        };
        tokio::task::spawn(backend.listen_on(receiver));
    }

    async fn listen_on(mut self, mut receiver: mpsc::UnboundedReceiver<DestinationAction>) {
        while let Some(action) = receiver.recv().await {
            match action {
                DestinationAction::Subscribe(borrowed_subscriber) => {
                    self.add_subscription(borrowed_subscriber);
                }
                DestinationAction::Unsubscribe(sub_id, borrowed_subscriber) => {
                    self.remove_subscription(sub_id, borrowed_subscriber);
                }
                DestinationAction::Send(message, borrowed_sender) => {
                    self.send(message, borrowed_sender);
                }
                DestinationAction::Close => {
                    log::info!("Closing destination '{}'", self.id);
                    break;
                }
            }
        }
    }

    fn add_subscription(&mut self, subscriber: Box<dyn BorrowedSubscriber>) {
        let id = SubscriptionId(Uuid::new_v4().to_string());

        self.subscriptions.insert(
            id.clone(),
            Subscription {
                id: id.clone(),
                subscriber,
            },
        );

        if let Some(subscription) = self.subscriptions.get(&id) {
            (&*subscription.subscriber)
                .borrow()
                .subscribe_callback(self.id.clone(), Ok(id));
        }
    }
    fn remove_subscription(
        &mut self,
        sub_id: SubscriptionId,
        subscriber: Box<dyn BorrowedSubscriber>,
    ) {
        self.subscriptions.remove(&sub_id);
        (&*subscriber)
            .borrow()
            .unsubscribe_callback(sub_id.clone(), Ok(sub_id));
    }

    fn send(&mut self, message: InboundMessage, borrowed_sender: Box<dyn BorrowedSender>) {
        let out_message = OutboundMessage {
            message_id: Uuid::new_v4().to_string(),
            body: message.body,
        };

        let mut subscriptions = self.subscriptions.values();
        let mut dead_subscriptions = Vec::new();

        while let Some(subscription) = subscriptions.next() {
            if let Err(_) = (&*subscription.subscriber)
                .borrow()
                .send(subscription.id.clone(), out_message.clone())
            {
                dead_subscriptions.push(subscription.id.clone());
            }
        }

        let mut dead_subscriptions = dead_subscriptions.into_iter();

        while let Some(sub_id) = dead_subscriptions.next() {
            self.subscriptions.remove(&sub_id);
        }
    }
}

impl InMemDestination {
    pub fn create(destinationId: &DestinationId) -> InMemDestination {
        let (sender, receiver) = mpsc::unbounded_channel();

        InMemDestinationBackend::start(destinationId.clone(), receiver);
        InMemDestination {
            id: destinationId.clone(),
            sender,
        }
    }
}

impl Drop for InMemDestination {
    fn drop(&mut self) {
        if let Err(_) = self.sender.send(DestinationAction::Close) {
            log::error!("Error closing destination {}.", self.id);
        }
    }
}

pub trait DestinationType = Destination + Send + Unpin + Sync + Clone + 'static;

#[derive(Clone)]
pub struct AsyncDestinations<D: DestinationType> {
    destinations: VersionedMap<DestinationId, D>,
    destination_factory: Arc<dyn Fn(&DestinationId) -> D + Sync + Send>,
}

impl<D: DestinationType> Destinations for AsyncDestinations<D> {
    fn subscribe<T: BorrowedSubscriber>(&self, destination: DestinationId, client: T) {
        tokio::task::spawn(
            self.destinations
                .get(
                    &destination,
                    self.destination_factory.clone()
                        as Arc<dyn Fn(&DestinationId) -> D + Sync + Send + 'static>,
                )
                .inspect(|destination| {
                    destination.subscribe(client);
                }),
        );
    }

    fn send<T: BorrowedSender>(
        &self,
        destination: DestinationId,
        message: InboundMessage,
        sender: T,
    ) {
        // Clone it, so that potential updates don't affect this copy
        // let destinations = self.cloned_dest();
        if let Some(mut destination) = self.destinations.get_if_present(&destination) {
            destination.send(message, sender);
        } else {
            sender.borrow().send_callback(
                message.sender_message_id,
                Err(StomperError::new(
                    format!("Unknown destination '{}'", destination).as_str(),
                )),
            );
        }
    }
}

impl<D: DestinationType> AsyncDestinations<D> {
    pub async fn start(
        destination_factory: Arc<dyn Fn(&DestinationId) -> D + Sync + Send>,
    ) -> AsyncDestinations<D> {
        AsyncDestinations {
            destinations: VersionedMap::new(),
            destination_factory,
        }
    }
}

#[cfg(test)]
mod test {

    use mockall::{mock, predicate::*};

    use super::{AsyncDestinations, InMemDestination};
    use crate::client::Client;
    use crate::destinations::{
        BorrowedSender, BorrowedSubscriber, Destination, DestinationId, Destinations,
        InboundMessage, OutboundMessage, Sender, Subscriber, SubscriptionId,
    };
    use crate::error::StomperError;

    use stomp_parser::model::*;

    use nonlocking::{AsyncMap, VersionedMap};

    use std::future::{self, Future};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, RwLock};

    use tokio::task::yield_now;
    use tokio::time::{sleep, Duration};

    use im::HashMap;
    use std::cell::RefCell;

    use log::info;

    mock! {
        TestDest{}

        impl Destination for TestDest {
            fn subscribe<T: BorrowedSubscriber>(&self, subscriber: T);
            fn unsubscribe<T: BorrowedSubscriber>(&self, sub_id: SubscriptionId, subscriber: T);

            fn send<T: BorrowedSender>(&self, message: InboundMessage, sender: T);

            fn close(&self);
        }

        impl Clone for TestDest {
            fn clone(&self) -> Self;
        }

        impl std::fmt::Debug for TestDest {
         fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
        }
    }

    mock! {
        TestClient{}

        impl Client for TestClient {
            fn connect_callback(&self, result: Result<(), StomperError>);
            fn into_sender(self: Arc<Self>) -> Arc<dyn Sender>;
            fn into_subscriber(self: Arc<Self>) -> Arc<dyn Subscriber>;
        }

        impl Sender for TestClient {
            fn send_callback(&self, sender_message_id: String, result: Result<(), StomperError>);
        }

        impl Subscriber for TestClient {
            fn subscribe_callback(
                &self,
                destination: DestinationId,
                result: Result<SubscriptionId, StomperError>,
            );

            fn unsubscribe_callback(
                &self,
                subscription: SubscriptionId,
                result: Result<SubscriptionId, StomperError>,
            );

            fn send(
                &self,
                subscription: SubscriptionId,
                message: OutboundMessage,
            ) -> Result<(), StomperError>;
        }

        impl std::fmt::Debug for TestClient {
         fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
        }
    }

    // Turn an Arc<Destination> into a Destination
    impl<A: Destination + std::marker::Sync> Destination for Arc<A> {
        fn subscribe<T>(&self, subscriber: T)
        where
            T: BorrowedSubscriber,
        {
            self.as_ref().subscribe(subscriber)
        }
        fn unsubscribe<T>(&self, sub: SubscriptionId, subscriber: T)
        where
            T: BorrowedSubscriber,
        {
            self.as_ref().unsubscribe(sub, subscriber)
        }
        fn send<T>(&self, message: InboundMessage, sender: T)
        where
            T: BorrowedSender,
        {
            self.as_ref().send(message, sender)
        }
        fn close(&self) {
            self.as_ref().close()
        }
    }

    fn into_sender(client: Arc<MockTestClient>) -> Arc<dyn Sender> {
        client
    }

    fn into_subscriber(client: Arc<MockTestClient>) -> Arc<dyn Subscriber> {
        client
    }

    fn create_client() -> Arc<MockTestClient> {
        let mut mock_client = Arc::new(MockTestClient::new());
        Arc::get_mut(&mut mock_client)
            .unwrap()
            .expect_into_subscriber()
            .returning(|| into_subscriber(Arc::new(MockTestClient::new())));
        mock_client
    }

    fn map_draining_factory(
        map: Arc<RwLock<HashMap<DestinationId, Arc<MockTestDest>>>>,
    ) -> Arc<dyn Fn(&DestinationId) -> Arc<MockTestDest> + Send + Sync + 'static> {
        Arc::new(move |id| map.try_write().unwrap().remove(&id).unwrap())
    }

    fn insert_subscribe_counting_dest(
        id: DestinationId,
        counter: Arc<AtomicUsize>,
        map: &mut Arc<RwLock<HashMap<DestinationId, Arc<MockTestDest>>>>,
    ) {
        let mut dest = MockTestDest::new();
        dest.expect_subscribe::<Arc<dyn Subscriber>>()
            .withf(move |_| {
                counter.fetch_add(1, Ordering::Release);
                true
            })
            .return_const(());
        map.try_write().unwrap().insert(id, Arc::new(dest));
    }

    #[tokio::test]
    async fn destinations_creates_on_subscribe() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut expected_destinations =
            Arc::<RwLock<HashMap<DestinationId, Arc<MockTestDest>>>>::default();

        let foo = DestinationId(String::from("foo"));

        insert_subscribe_counting_dest(foo.clone(), count.clone(), &mut expected_destinations);

        let destinations = AsyncDestinations::<Arc<MockTestDest>>::start(map_draining_factory(
            expected_destinations.clone(),
        ))
        .await;

        let client = create_client();

        destinations.subscribe(foo, into_subscriber(client));

        yield_now().await;

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

        insert_subscribe_counting_dest(foo.clone(), count.clone(), &mut expected_destinations);

        let bar = DestinationId(String::from("bar"));

        insert_subscribe_counting_dest(bar.clone(), count.clone(), &mut expected_destinations);

        let destinations = AsyncDestinations::<Arc<MockTestDest>>::start(map_draining_factory(
            expected_destinations.clone(),
        ))
        .await;

        destinations.subscribe(foo, into_subscriber(create_client()));

        yield_now().await;

        destinations.subscribe(bar, into_subscriber(create_client()));

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

        insert_subscribe_counting_dest(foo.clone(), count.clone(), &mut expected_destinations);

        let destinations = AsyncDestinations::<Arc<MockTestDest>>::start(map_draining_factory(
            expected_destinations.clone(),
        ))
        .await;

        destinations.subscribe(foo.clone(), into_subscriber(create_client()));

        destinations.subscribe(foo.clone(), into_subscriber(create_client()));

        destinations.subscribe(foo.clone(), into_subscriber(create_client()));

        destinations.subscribe(foo, into_subscriber(create_client()));

        yield_now().await;

        // Check all expected destinations were "created"
        assert_eq!(0, expected_destinations.try_read().unwrap().len());

        // check that subscribe was called four times
        assert_eq!(4, count.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn destination_calls_subscribe_callback() {
        let foo = DestinationId(String::from("foo"));
        let destination = InMemDestination::create(&foo);

        let mut client = create_client();
        Arc::get_mut(&mut client)
            .unwrap()
            .expect_subscribe_callback()
            .times(1)
            .withf(move |dest_id, result| *dest_id == foo && result.is_ok())
            .return_const(());

        destination.subscribe(into_subscriber(client.clone()));
        drop(destination);

        yield_now().await;
        //Arc::get_mut(&mut client).unwrap().checkpoint();
    }

    #[tokio::test]
    async fn destination_sends_to_subscribers() {
        let foo = DestinationId(String::from("foo"));
        let destination = InMemDestination::create(&foo);

        let sub_id = Arc::new(RwLock::new(Some(SubscriptionId(String::from("false")))));

        let sub_id_for_closure = sub_id.clone();

        let mut client = create_client();
        Arc::get_mut(&mut client)
            .unwrap()
            .expect_subscribe_callback()
            .times(1)
            .withf(move |dest_id, result| *dest_id == foo && result.is_ok())
            .returning(move |_, result| {
                sub_id_for_closure
                    .try_write()
                    .unwrap()
                    .replace(result.unwrap());
                ()
            });

        Arc::get_mut(&mut client)
            .unwrap()
            .expect_send_callback()
            .return_const(());

        Arc::get_mut(&mut client)
            .unwrap()
            .expect_send()
            .times(1)
            .withf(move |subscription, message| {
                String::from_utf8_lossy(&message.body) == "Hello, World 42"
                    && *subscription == sub_id.try_write().unwrap().clone().unwrap()
            })
            .return_const(Ok(()));

        destination.subscribe(into_subscriber(client.clone()));
        destination.send(
            InboundMessage {
                sender_message_id: String::from("my_msg"),
                body: "Hello, World 42".as_bytes().to_owned(),
            },
            into_sender(client.clone()),
        );

        yield_now().await;
    }
}
