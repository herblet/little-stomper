use std::ops::Deref;

use crate::error::StomperError;

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

pub trait Sender: Send + Sync + std::fmt::Debug {
    fn send_callback(
        &self,
        sender_message_id: Option<MessageId>,
        result: Result<MessageId, StomperError>,
    );
}

/// A destinations is a identifiable resource that clients can subscribe to, and send messages to
pub trait Destination: Send + Clone {
    type Client: crate::client::Client;

    fn subscribe<S: Subscriber + 'static, D: Deref<Target = S> + Send + Clone + 'static>(
        &self,
        sender_subscription_id: Option<SubscriptionId>,
        subscriber: D,
        client: &Self::Client,
    );

    fn unsubscribe<S: Subscriber + 'static, D: Deref<Target = S> + Send + Clone + 'static>(
        &self,
        sub_id: SubscriptionId,
        subscriber: D,
        client: &Self::Client,
    );

    fn send<S: Sender + 'static, D: Deref<Target = S> + Send + Clone + 'static>(
        &self,
        message: InboundMessage,
        sender: D,
        client: &Self::Client,
    );

    fn close(&self);
}

pub trait Destinations: Send + Clone {
    type Client: crate::client::Client;

    fn subscribe<S: Subscriber + 'static, D: Deref<Target = S> + Clone + Send + 'static>(
        &self,
        destination: DestinationId,
        sender_subscription_id: Option<SubscriptionId>,
        subscriber: D,
        client: &Self::Client,
    );

    fn unsubscribe<S: Subscriber + 'static, D: Deref<Target = S> + Clone + Send + 'static>(
        &self,
        destination: DestinationId,
        destination_subscription_id: SubscriptionId,
        subscriber: D,
        client: &Self::Client,
    );

    /// Send a message to this destination and, by implication, all clients subscribed to it
    fn send<S: Sender + 'static, D: Deref<Target = S> + Clone + Send + 'static>(
        &self,
        destination: DestinationId,
        message: InboundMessage,
        sender: D,
        client: &Self::Client,
    );
}
