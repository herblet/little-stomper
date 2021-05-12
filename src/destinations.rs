use crate::client::Client;
use crate::error::StomperError;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use stomp_parser::model::{SendFrame, SubscribeFrame};

// An action that a destination can perform
pub enum DestinationAction {
    Subscribe(SubscribeFrame, Arc<dyn Client>),
    Send(SendFrame),
    Quit,
}

/// A destinations is a identifiable resource that clients can subscribe to, and send messages to
pub trait Destination: Send + Clone {
    /// Performs the provided action against this destination, returning a Future on completion
    /// of that action.
    ///
    /// *Note*: The Destination is captured until the future is completed.
    fn perform_action<'a>(
        &'a mut self,
        action: DestinationAction,
    ) -> Pin<Box<dyn Future<Output = Result<(), StomperError>> + 'a + Send>>;
}

pub trait Destinations: Send + Clone {
    /// Subscribe the provided client to this destination, using the parameters from the
    /// provided frame
    fn subscribe(
        &self,
        frame: SubscribeFrame,
        client: Arc<dyn Client>,
    ) -> Pin<Box<dyn Future<Output = Result<(), StomperError>> + Send + 'static>>;

    /// Send a message to this destination and, by implication, all clients subscribed to it
    fn send_message(
        &self,
        frame: SendFrame,
    ) -> Pin<Box<dyn Future<Output = Result<(), StomperError>> + Send + 'static>>;
}
