use crate::destinations::{Sender, Subscriber};
use crate::error::StomperError;
use std::sync::Arc;

/// A proxy for a client which can subscribe to destinations, receive messages and send messages.
///
/// Note that a client must also implement [destinations::Subscriber](crate::destinations::Subscriber) and [destinations::Sender](crate::destinations::Sender),
/// which define the bulk of the API.
pub trait Client: Subscriber + Sender + Sync + Send {
    /// A callback which is called when a client has connected, with the result of processing to date.
    fn connect_callback(&self, result: Result<(), StomperError>);

    /// Allows error messages to be send to the client
    fn error(&self, message: &str);

    /// Exposes self as a Sender.
    fn into_sender(self: Arc<Self>) -> Arc<dyn Sender>;

    /// Exposes self as a Subscriber.
    fn into_subscriber(self: Arc<Self>) -> Arc<dyn Subscriber>;

    fn send_heartbeat(self: Arc<Self>);
}
