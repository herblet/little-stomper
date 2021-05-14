use crate::destinations::{Sender, Subscriber};
use crate::error::StomperError;
use std::sync::Arc;

// type ClientOutputSink = Sink<ServerFrame>;
// type ClientInputSink = Sink<ClientFrame>;

/// Represents a client which can receive messages.
pub trait Client: Subscriber + Sender + Sync + Send {
    fn connect_callback(&self, result: Result<(), StomperError>);
    fn into_sender(self: Arc<Self>) -> Arc<dyn Sender>;
    fn into_subscriber(self: Arc<Self>) -> Arc<dyn Subscriber>;
}
