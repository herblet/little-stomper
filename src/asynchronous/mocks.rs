#![cfg(test)]

use std::sync::Arc;

use crate::destinations::*;
use crate::error::StomperError;

use mockall::{mock, predicate::*};

mock! {
    pub TestSubscriber {}


    impl Subscriber for TestSubscriber {
        fn subscribe_callback(
            &self,
            destination: DestinationId,
            subscriber_sub_id: Option<SubscriptionId>,
            result: Result<SubscriptionId, StomperError>,
        );

        fn unsubscribe_callback(
            &self,
            subscriber_sub_id: Option<SubscriptionId>,
            result: Result<SubscriptionId, StomperError>,
        );

        fn send(
            &self,
            subscription: SubscriptionId,
            subscriber_sub_id: Option<SubscriptionId>,
            message: OutboundMessage,
        ) -> Result<(), StomperError>;
    }

    impl std::fmt::Debug for TestClient {
     fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
    }
}

mock! {
    pub TestSender {}

    impl Sender for TestSender {
        fn send_callback(&self, sender_message_id: Option<MessageId>, result: Result<MessageId, StomperError>);
    }

    impl std::fmt::Debug for TestSender {
     fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
    }
}

mock! {
    pub TestDest{}

    impl Destination for TestDest {
        fn subscribe<T: BorrowedSubscriber>(&self, subscriber_sub_id: Option<SubscriptionId>, subscriber: T);
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

pub fn into_sender(sender: Arc<MockTestSender>) -> Arc<dyn Sender + 'static> {
    sender
}

pub fn create_sender() -> Arc<MockTestSender> {
    Arc::new(MockTestSender::new())
}

pub fn into_subscriber(subscriber: Arc<MockTestSubscriber>) -> Arc<dyn Subscriber + 'static> {
    subscriber
}

pub fn create_subscriber() -> Arc<MockTestSubscriber> {
    Arc::new(MockTestSubscriber::new())
}
