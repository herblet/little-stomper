use std::collections::HashMap;

use crate::destinations::*;
use crate::error::StomperError;

use tokio::sync::mpsc;
use tokio::task;

use uuid::Uuid;

struct Subscription {
    id: SubscriptionId,
    subscriber_sub_id: Option<SubscriptionId>,
    subscriber: Box<dyn BorrowedSubscriber>,
}

impl Subscription {
    pub fn send(&self, message: OutboundMessage) -> Result<(), StomperError> {
        (&*self.subscriber)
            .borrow()
            .send(self.id.clone(), self.subscriber_sub_id.clone(), message)
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

/// An action that a destination can perform
enum DestinationAction {
    Subscribe(Option<SubscriptionId>, Box<dyn BorrowedSubscriber>),
    Unsubscribe(SubscriptionId, Box<dyn BorrowedSubscriber>),
    Send(InboundMessage, Box<dyn BorrowedSender>),
    Close,
}

/// A destination that simply stores its subscriptions in memory
#[derive(Clone)]
pub struct InMemDestination {
    id: DestinationId,
    sender: mpsc::UnboundedSender<DestinationAction>,
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
    fn subscribe<T: BorrowedSubscriber>(
        &self,
        sender_subscription_id: Option<SubscriptionId>,
        subscriber: T,
    ) {
        match self.perform_action(DestinationAction::Subscribe(
            sender_subscription_id,
            Box::new(subscriber),
        )) {
            Err(err) => {
                if let mpsc::error::SendError(DestinationAction::Subscribe(
                    sender_subscription_id,
                    subscriber,
                )) = err
                {
                    (&*subscriber).borrow().subscribe_callback(
                        self.id.clone(),
                        sender_subscription_id,
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
                if let mpsc::error::SendError(DestinationAction::Unsubscribe(_, subscriber)) = err {
                    (&*subscriber)
                        .borrow()
                        .unsubscribe_callback(None, Err(StomperError::new("Unsubscribe failed")));
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
        task::spawn(backend.listen_on(receiver));
    }

    async fn listen_on(mut self, mut receiver: mpsc::UnboundedReceiver<DestinationAction>) {
        while let Some(action) = receiver.recv().await {
            match action {
                DestinationAction::Subscribe(subscriber_sub_id, borrowed_subscriber) => {
                    self.add_subscription(subscriber_sub_id, borrowed_subscriber);
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

    fn add_subscription(
        &mut self,
        subscriber_sub_id: Option<SubscriptionId>,
        subscriber: Box<dyn BorrowedSubscriber>,
    ) {
        let id = SubscriptionId(Uuid::new_v4().to_string());

        self.subscriptions.insert(
            id.clone(),
            Subscription {
                id: id.clone(),
                subscriber_sub_id,
                subscriber,
            },
        );

        if let Some(subscription) = self.subscriptions.get(&id) {
            (&*subscription.subscriber).borrow().subscribe_callback(
                self.id.clone(),
                subscription.subscriber_sub_id.clone(),
                Ok(id),
            );
        }
    }
    fn remove_subscription(
        &mut self,
        sub_id: SubscriptionId,
        subscriber: Box<dyn BorrowedSubscriber>,
    ) {
        let subscription = self.subscriptions.remove(&sub_id);
        (&*subscriber).borrow().unsubscribe_callback(
            subscription.and_then(|subscription| subscription.subscriber_sub_id),
            Ok(sub_id),
        );
    }

    fn send(&mut self, message: InboundMessage, sender: Box<dyn BorrowedSender>) {
        let message_id = MessageId(Uuid::new_v4().to_string());

        let out_message = OutboundMessage {
            message_id: message_id.clone(),
            body: message.body,
        };

        let mut subscriptions = self.subscriptions.values();
        let mut dead_subscriptions = Vec::new();

        while let Some(subscription) = subscriptions.next() {
            if let Err(_) = subscription.send(out_message.clone()) {
                dead_subscriptions.push(subscription.id.clone());
            }
        }

        let mut dead_subscriptions = dead_subscriptions.into_iter();

        while let Some(sub_id) = dead_subscriptions.next() {
            self.subscriptions.remove(&sub_id);
        }

        sender
            .borrow()
            .send_callback(message.sender_message_id, Ok(message_id));
    }
}

impl InMemDestination {
    pub fn create(destination_id: &DestinationId) -> InMemDestination {
        let (sender, receiver) = mpsc::unbounded_channel();

        InMemDestinationBackend::start(destination_id.clone(), receiver);
        InMemDestination {
            id: destination_id.clone(),
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

#[cfg(test)]
mod test {
    use super::super::mocks::MockTestClient;
    use super::*;
    use std::sync::{Arc, RwLock};

    use tokio::task::yield_now;

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
    #[tokio::test]
    async fn destination_calls_subscribe_callback() {
        let foo = DestinationId::from("foo");
        let sub_id = SubscriptionId::from("bar");
        let destination = InMemDestination::create(&foo);

        let mut client = create_client();
        let sub_id_for_closure = sub_id.clone();
        Arc::get_mut(&mut client)
            .unwrap()
            .expect_subscribe_callback()
            .times(1)
            .withf(move |dest_id, sub_id, result| {
                *dest_id == foo
                    && result.is_ok()
                    && sub_id
                        .as_ref()
                        .map(|sub_id| *sub_id == sub_id_for_closure)
                        .unwrap_or(false)
            })
            .return_const(());

        destination.subscribe(Some(sub_id), into_subscriber(client.clone()));
        drop(destination);

        yield_now().await;
        //Arc::get_mut(&mut client).unwrap().checkpoint();
    }

    #[tokio::test]
    async fn destination_calls_unsubscribe_callback() {
        let foo = DestinationId::from("foo");
        let subscriber_sub_id = SubscriptionId::from("bar");
        let subscriber_sub_id_for_closure = subscriber_sub_id.clone();

        let sub_id = Arc::new(RwLock::new(None));
        let sub_id_for_closure = sub_id.clone();

        let destination = InMemDestination::create(&foo);

        let mut client = create_client();
        Arc::get_mut(&mut client)
            .unwrap()
            .expect_subscribe_callback()
            .times(1)
            .withf(move |dest_id, received_subcriber_sub_id, result| {
                sub_id_for_closure
                    .try_write()
                    .unwrap()
                    .replace(result.as_ref().ok().unwrap().clone());

                *dest_id == foo
                    && received_subcriber_sub_id
                        .as_ref()
                        .map(|sub_id| *sub_id == subscriber_sub_id_for_closure)
                        .unwrap_or(false)
            })
            .return_const(());
        Arc::get_mut(&mut client)
            .unwrap()
            .expect_unsubscribe_callback()
            .times(1)
            .return_const(());

        destination.subscribe(Some(subscriber_sub_id), into_subscriber(client.clone()));
        yield_now().await;
        destination.unsubscribe(
            sub_id.try_read().unwrap().as_ref().unwrap().clone(),
            into_subscriber(client.clone()),
        );
        yield_now().await;
    }

    #[tokio::test]
    async fn destination_calls_send_callback() {
        let foo = DestinationId(String::from("foo"));
        let sub_id = SubscriptionId::from("bar");

        let destination = InMemDestination::create(&foo);

        let mut client = create_client();
        Arc::get_mut(&mut client)
            .unwrap()
            .expect_send_callback()
            .times(1)
            .return_const(());

        destination.subscribe(Some(sub_id), into_subscriber(client.clone()));

        destination.send(
            InboundMessage {
                sender_message_id: Some(MessageId::from("msg-1")),
                body: "Slartibartfast rules".as_bytes().to_owned(),
            },
            into_sender(client),
        );

        yield_now().await;
        //Arc::get_mut(&mut client).unwrap().checkpoint();
    }

    #[tokio::test]
    async fn destination_sends_to_subscribers() {
        let foo = DestinationId(String::from("foo"));
        let destination = InMemDestination::create(&foo);

        let sub_id = Arc::new(RwLock::new(Some(SubscriptionId(String::from("false")))));
        let sender_sub_id = SubscriptionId::from("false");

        let sub_id_for_closure = sub_id.clone();
        let sender_sub_id_for_closure = sender_sub_id.clone();

        let mut client = create_client();
        Arc::get_mut(&mut client)
            .unwrap()
            .expect_subscribe_callback()
            .times(1)
            .withf(move |dest_id, _sub_id, result| *dest_id == foo && result.is_ok())
            .returning(move |_, _, result| {
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
            .withf(move |subscription, subscriber_sub_id, message| {
                String::from_utf8_lossy(&message.body) == "Hello, World 42"
                    // The subscriptionId is the one from the subscribecallback
                    && *subscription == sub_id.try_write().unwrap().clone().unwrap()
                    && subscriber_sub_id.as_ref().map(|received_sender_sub_id| *received_sender_sub_id ==  sender_sub_id_for_closure).unwrap_or(false)
            })
            .return_const(Ok(()));

        destination.subscribe(Some(sender_sub_id), into_subscriber(client.clone()));
        destination.send(
            InboundMessage {
                sender_message_id: Some(MessageId::from("my_msg")),
                body: "Hello, World 42".as_bytes().to_owned(),
            },
            into_sender(client.clone()),
        );

        yield_now().await;
    }
}