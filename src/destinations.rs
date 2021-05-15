use crate::error::StomperError;
use std::borrow::Borrow;

string_id_class!(SubscriptionId);

string_id_class!(DestinationId);

string_id_class!(MessageId);

pub struct InboundMessage {
    pub sender_message_id: Option<MessageId>,
    pub body: Vec<u8>,
}

#[derive(Clone)]
pub struct OutboundMessage {
    pub destination: DestinationId,
    pub message_id: MessageId,
    pub body: Vec<u8>,
}

pub trait Subscriber: Send + Sync + std::fmt::Debug {
    fn subscribe_callback(
        &self,
        destination: DestinationId,
        suscriber_sub_id: Option<SubscriptionId>,
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
        suscriber_sub_id: Option<SubscriptionId>,
        message: OutboundMessage,
    ) -> Result<(), StomperError>;
}

pub trait BorrowedSubscriber = Borrow<dyn Subscriber> + Send + 'static;

pub trait Sender: Send + Sync + std::fmt::Debug {
    fn send_callback(
        &self,
        sender_message_id: Option<MessageId>,
        result: Result<MessageId, StomperError>,
    );
}

pub trait BorrowedSender = Borrow<dyn Sender> + Send + 'static;

/// A destinations is a identifiable resource that clients can subscribe to, and send messages to
pub trait Destination: Send + Clone {
    fn subscribe<T: BorrowedSubscriber>(
        &self,
        sender_subscription_id: Option<SubscriptionId>,
        subscriber: T,
    );
    fn unsubscribe<T: BorrowedSubscriber>(&self, sub_id: SubscriptionId, subscriber: T);

    fn send<T: BorrowedSender>(&self, message: InboundMessage, sender: T);

    fn close(&self);
}

pub trait Destinations: Send + Clone {
    fn subscribe<T: BorrowedSubscriber>(
        &self,
        destination: DestinationId,
        sender_subscription_id: Option<SubscriptionId>,
        client: T,
    );

    /// Send a message to this destination and, by implication, all clients subscribed to it
    fn send<T: BorrowedSender>(
        &self,
        destination: DestinationId,
        message: InboundMessage,
        sender: T,
    );
}
