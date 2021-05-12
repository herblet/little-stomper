use crate::error::StomperError;
use stomp_parser::model::ServerFrame;

use futures::Sink;

// type ClientOutputSink = Sink<ServerFrame>;
// type ClientInputSink = Sink<ClientFrame>;

/// Represents a client which can receive messages.
pub trait Client: Sync + Send {
    fn send(&self, frame: ServerFrame) -> Result<(), StomperError>;
}
