use crate::client::Client;
use crate::destinations::{Destination, DestinationAction, Destinations};
use crate::error::StomperError;

use nonlocking::{AsyncMap, FactoryBorrow, VersionedMap};

use stomp_parser::model::*;

use std::borrow::Borrow;
use std::pin::Pin;
use std::sync::Arc;

use futures::FutureExt;
use std::future::{ready, Future};

use tokio::sync::mpsc::{self, UnboundedSender};

use log::info;

struct Subscription {
    id: String,
    client: Arc<dyn Client>,
}

impl Subscription {
    pub fn send_message(&mut self, message: &DestinationMessage) -> Result<(), StomperError> {
        let (content_length, body) = match &message.source_frame.body() {
            None => (0u32, "".to_owned()),
            Some(bytes) => (
                bytes.len() as u32,
                std::str::from_utf8(&bytes).unwrap().to_owned(),
            ),
        };
        if let Some(passed_length) = &message.source_frame.content_length {
            if *passed_length.value() != content_length as u32 {
                info!(
                    "Strange: passed content-length {} not equal to actual length of content {}.",
                    passed_length, content_length
                )
            }
        }

        let raw_body = body.as_bytes().to_owned();

        let mut message = MessageFrame::new(
            MessageIdValue::new(message.message_id.clone()),
            DestinationValue::new(message.source_frame.destination.value().clone()),
            SubscriptionValue::new(self.id.clone()),
            Some(message.source_frame.content_type.as_ref().map_or_else(
                || ContentTypeValue::new("text/plain".to_owned()),
                |value| value.clone(),
            )),
            Some(ContentLengthValue::new(content_length)),
            (0, raw_body.len()),
        );

        message.set_raw(raw_body);

        self.client.send(ServerFrame::Message(message))
    }
}

struct DestinationMessage {
    message_id: String,
    source_frame: SendFrame,
}

/// A destination that simply stores its subscriptions in memory
#[derive(Clone)]
pub struct InMemDestination {
    path: String,
    sender: tokio::sync::mpsc::UnboundedSender<DestinationAction>,
}

impl Destination for InMemDestination {
    fn perform_action<'a>(
        &'a mut self,
        action: DestinationAction,
    ) -> Pin<Box<dyn Future<Output = Result<(), StomperError>> + 'a + Send>> {
        ready(self.sender.send(action).map_err(|_| {
            StomperError::new(format!("Unable to send to destination '{}'", self.path).as_str())
        }))
        .boxed()
    }
}

struct InMemDestinationBackend {
    subscriptions: Vec<Subscription>,
    counter: u64,
}

impl InMemDestinationBackend {
    fn start(mut receiver: mpsc::UnboundedReceiver<DestinationAction>) {
        tokio::task::spawn(async move {
            let mut backend = InMemDestinationBackend {
                subscriptions: Vec::new(),
                counter: 0,
            };
            while let Some(action) = receiver.recv().await {
                match action {
                    DestinationAction::Subscribe(frame, client) => {
                        backend.add_subscription(frame, client);
                    }
                    DestinationAction::Send(frame) => {
                        backend.send(frame);
                    }
                    DestinationAction::Quit => {
                        todo!();
                    }
                }
            }
        });
    }

    fn add_subscription<'a>(&'a mut self, frame: SubscribeFrame, client: Arc<dyn Client>) {
        self.subscriptions.push(Subscription {
            id: frame.id.value().clone(),
            client,
        });
        let id = self.subscriptions.len() - 1;
    }

    fn send<'a>(
        &'a mut self,
        frame: SendFrame,
    ) -> Pin<Box<dyn Future<Output = Result<(), StomperError>> + 'a + Send>> {
        let message = DestinationMessage {
            message_id: self.counter.to_string(),
            source_frame: frame,
        };

        self.counter += 1;

        // Need to modify each subscription (message count) and also remove those that
        // are 'dead'; to do this, swap out subscription, then map it to the (potentially reduced) result; and put it back in.
        let mut index = 0;

        while index < self.subscriptions.len() {
            let sub = &mut self.subscriptions[index];

            match sub.send_message(&message) {
                Err(_) => {
                    // This subscription is dead...remove it
                    self.subscriptions.swap_remove(index);

                    // Don't increase index since the item at the current index
                    // is now a new one
                }
                Ok(_) => {
                    index += 1;
                }
            }
        }

        ready(Ok(())).boxed()
    }
}

impl InMemDestination {
    pub fn create(path: &String) -> InMemDestination {
        let (sender, receiver) = mpsc::unbounded_channel();

        InMemDestinationBackend::start(receiver);
        InMemDestination {
            path: path.clone(),
            sender,
        }
    }
}

pub trait DestinationType = Destination + Send + Unpin + Sync + Clone + 'static;

#[derive(Clone)]
pub struct AsyncDestinations<D: DestinationType> {
    // No locking because only used from Destinations own task
    destinations: VersionedMap<String, D>,
    destination_factory: Arc<dyn Fn(&String) -> D + Sync + Send>,
}

impl<D: DestinationType> Destinations for AsyncDestinations<D> {
    fn subscribe(
        &self,
        frame: SubscribeFrame,
        client: Arc<(dyn Client + 'static)>,
    ) -> Pin<Box<dyn Future<Output = Result<(), StomperError>> + Send + 'static>> {
        let destination = frame.destination.clone();

        self.destinations
            .get(
                &destination.value(),
                self.destination_factory.clone()
                    as Arc<dyn Fn(&String) -> D + Sync + Send + 'static>,
            )
            .map(|mut destination| {
                destination.perform_action(DestinationAction::Subscribe(frame, client));
                Ok(())
            })
            .boxed()
    }

    fn send_message(
        &self,
        frame: SendFrame,
    ) -> Pin<Box<dyn Future<Output = Result<(), StomperError>> + Send + 'static>> {
        let destination = frame.destination.clone();

        // Clone it, so that potential updates don't affect this copy
        // let destinations = self.cloned_dest();
        if let Some(mut destination) = self.destinations.get_if_present(&destination.value()) {
            destination.perform_action(DestinationAction::Send(frame));
        }

        ready(Ok(())).boxed()
    }
}

impl<D: DestinationType> AsyncDestinations<D> {
    pub async fn start(
        destination_factory: Arc<dyn Fn(&String) -> D + Sync + Send>,
    ) -> AsyncDestinations<D> {
        AsyncDestinations {
            destinations: VersionedMap::new(),
            destination_factory,
        }
    }
}

#[cfg(test)]
mod test {

    use super::AsyncDestinations;
    use crate::client::Client;
    use crate::destinations::{Destination, DestinationAction, Destinations};
    use crate::error::StomperError;

    use stomp_parser::model::*;

    use std::future::{self, Future};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tokio::task::yield_now;
    use tokio::time::{sleep, Duration};

    use im::HashMap;

    use log::info;

    #[derive(Default, Clone)]
    struct TestDest {
        ctr: usize,
    }

    impl Destination for TestDest {
        fn perform_action<'a>(
            &'a mut self,
            action: DestinationAction,
        ) -> std::pin::Pin<
            std::boxed::Box<
                (dyn futures::Future<Output = std::result::Result<(), StomperError>>
                     + std::marker::Send
                     + 'a),
            >,
        > {
            self.ctr += 1;
            println!("Destination.perform_action: {}", self.ctr);
            Box::pin(future::ready(Ok(())))
        }
    }

    struct TestClient {}

    impl Client for TestClient {
        fn send(&self, _: ServerFrame) -> Result<(), StomperError> {
            todo!()
        }
    }

    async fn create_destinations() -> (Arc<AtomicUsize>, AsyncDestinations<TestDest>) {
        println!("Creating destinations");
        let calls = Arc::new(AtomicUsize::new(0));

        (
            calls.clone(),
            AsyncDestinations::<TestDest>::start(Arc::new(move |destination| {
                println!("Creating new destination");
                calls.fetch_add(1, Ordering::Relaxed);
                TestDest::default()
            }))
            .await,
        )
    }
    async fn subscribe(destinations: &AsyncDestinations<TestDest>, path: &str) {
        let frame = SubscribeFrame::new(
            DestinationValue::new(path.to_owned()),
            IdValue::new("Hello".to_owned()),
            Some(AckValue::new(AckType::Auto)),
            None,
            Vec::new(),
        );

        if let Err(_) = destinations.subscribe(frame, Arc::new(TestClient {})).await {
            panic!("subscribe failed!");
        }

        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn it_creates_on_subscribe() {
        let (calls, destinations) = create_destinations().await;

        subscribe(&destinations, "foo").await;

        assert_eq!(1, calls.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn it_creates_each_destination() {
        let (calls, destinations) = create_destinations().await;

        subscribe(&destinations, "foo").await;
        subscribe(&destinations, "bar").await;

        // Should have created twice - once for each destination
        assert_eq!(2, calls.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn it_reuses_on_subsequent_subscribe() {
        let _ = env_logger::try_init();

        info!("Foo");
        let (calls, destinations) = create_destinations().await;

        subscribe(&destinations, "foo").await;
        subscribe(&destinations, "foo").await;
        subscribe(&destinations, "foo").await;
        subscribe(&destinations, "foo").await;

        // Should only have created once
        assert_eq!(1, calls.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn gets_future() {
        assert_eq!(5, get_future().await);
    }

    #[derive(Clone)]
    struct TestStruct {
        value: String,
    }

    #[test]
    fn test_make_mut() {
        let mut x = Arc::new(TestStruct {
            value: "foobar".to_owned(),
        });

        let y = x.clone();

        assert_eq!(2, Arc::strong_count(&x));

        Arc::make_mut(&mut x).value = "barfoo".to_owned();

        assert_eq!("foobar", y.value);
        assert_eq!("barfoo", x.value);

        assert_eq!(1, Arc::strong_count(&x));
        assert_eq!(1, Arc::strong_count(&y));
    }

    #[test]
    fn test_im_Map() {
        let mut map = HashMap::<String, String>::default();
        map.insert("foo".to_owned(), "bar".to_owned());

        let clone = map.clone();

        map.insert("foo2".to_owned(), "bar2".to_owned());

        assert_eq!("bar2", map.get("foo2").unwrap());
        assert_eq!("bar", map.get("foo").unwrap());
        assert_eq!("bar", clone.get("foo").unwrap());
        assert_eq!(false, clone.contains_key("foo2"));
    }
    fn get_future() -> impl Future<Output = u8> {
        async { 5 }
    }
}
