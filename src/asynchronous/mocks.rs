#![cfg(test)]

use std::sync::Arc;

use crate::client::*;
use crate::destinations::*;
use crate::error::StomperError;

use mockall::{mock, predicate::*};

mock! {
    pub TestClient{}

    impl Client for TestClient {
        fn connect_callback(&self, result: Result<(), StomperError>);
        fn into_sender(self: Arc<Self>) -> Arc<dyn Sender>;
        fn into_subscriber(self: Arc<Self>) -> Arc<dyn Subscriber>;
        fn error(&self, message: &str);
        fn send_heartbeat(self: Arc<Self>);
    }

    impl Sender for TestClient {
        fn send_callback(&self, sender_message_id: Option<MessageId>, result: Result<MessageId, StomperError>);
    }

    impl Subscriber for TestClient {
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
