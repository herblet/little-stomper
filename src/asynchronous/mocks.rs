#![cfg(test)]

use crate::client::DefaultClient;
use crate::destinations::*;
use crate::error::StomperError;
use std::ops::Deref;
use std::sync::Arc;

use mockall::{mock, predicate::*};

mock! {
    pub TestSubscriber {}

    impl Clone for TestSubscriber {
         fn clone(&self) -> Self;
    }

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

    impl std::fmt::Debug for TestSubscriber {
     fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
    }
}

mock! {
    pub TestSender {}

     impl Clone for TestSender {
         fn clone(&self) -> Self;
    }

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
        type Client = DefaultClient;

        fn subscribe<S: Subscriber + 'static, D: Deref<Target = S> + Send + Clone + 'static>(&self, subscriber_sub_id: Option<SubscriptionId>, subscriber: D, client: &DefaultClient);
        fn unsubscribe<S: Subscriber + 'static, D: Deref<Target = S> + Send + Clone + 'static>(&self, sub_id: SubscriptionId, subscriber: D, client: &DefaultClient);

        fn send<S: Sender + 'static, D: Deref<Target = S> + Send + Clone + 'static>(&self, message: InboundMessage, sender: D, client: &DefaultClient);

        fn close(&self);
    }

    impl Clone for TestDest {
        fn clone(&self) -> Self;
    }

    impl std::fmt::Debug for TestDest {
     fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
    }
}

pub fn create_sender() -> Arc<MockTestSender> {
    Arc::new(MockTestSender::new())
}

pub fn create_subscriber() -> Arc<MockTestSubscriber> {
    Arc::new(MockTestSubscriber::new())
}
