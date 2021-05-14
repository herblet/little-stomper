use crate::client::Client;
use crate::error::StomperError;
use std::borrow::Borrow;
use std::sync::Arc;

use stomp_parser::model::{SendFrame, SubscribeFrame};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SubscriptionId(pub String);

impl std::fmt::Display for SubscriptionId {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::result::Result<(), std::fmt::Error> {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct DestinationId(pub String);

impl std::fmt::Display for DestinationId {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::result::Result<(), std::fmt::Error> {
        formatter.write_str(&self.0)
    }
}

pub struct InboundMessage {
    pub sender_message_id: String,
    pub body: Vec<u8>,
}

#[derive(Clone)]
pub struct OutboundMessage {
    pub message_id: String,
    pub body: Vec<u8>,
}

pub trait Subscriber: Send + Sync + std::fmt::Debug {
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

pub trait BorrowedSubscriber = Borrow<dyn Subscriber> + Send + 'static;

pub trait Sender: Send + Sync + std::fmt::Debug {
    fn send_callback(&self, sender_message_id: String, result: Result<(), StomperError>);
}

pub trait BorrowedSender = Borrow<dyn Sender> + Send + 'static;

/// A destinations is a identifiable resource that clients can subscribe to, and send messages to
pub trait Destination: Send + Clone {
    fn subscribe<T: BorrowedSubscriber>(&self, subscriber: T);
    fn unsubscribe<T: BorrowedSubscriber>(&self, sub_id: SubscriptionId, subscriber: T);

    fn send<T: BorrowedSender>(&self, message: InboundMessage, sender: T);

    fn close(&self);
}

pub trait Destinations: Send + Clone {
    fn subscribe<T: BorrowedSubscriber>(&self, destination: DestinationId, client: T);

    /// Send a message to this destination and, by implication, all clients subscribed to it
    fn send<T: BorrowedSender>(
        &self,
        destination: DestinationId,
        message: InboundMessage,
        sender: T,
    );
}
