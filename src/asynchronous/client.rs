use crate::client::Client;
use crate::destinations::{
    DestinationId, MessageId, OutboundMessage, Sender, Subscriber, SubscriptionId,
};
use crate::error::StomperError;
use crate::Destinations;
use crate::FrameHandler;
use crate::FrameHandlerImpl;
use crate::StompClient;

use futures::{FutureExt, Sink, Stream, StreamExt, TryFutureExt, TryStreamExt};
use log::info;
use std::future::ready;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use stomp_parser::model::frames::client::parsers::client_frame;
use stomp_parser::model::*;
use tokio_stream::wrappers::UnboundedReceiverStream;

use tokio::sync::mpsc::{self, UnboundedSender};

#[derive(Debug)]
pub struct AsyncStompClient {
    sender: UnboundedSender<ServerFrame>,
}

impl Subscriber for AsyncStompClient {
    fn subscribe_callback(
        &self,
        _: DestinationId,
        _: Option<SubscriptionId>,
        _: Result<SubscriptionId, StomperError>,
    ) {
        todo!()
    }
    fn unsubscribe_callback(
        &self,
        _: Option<SubscriptionId>,
        _: std::result::Result<SubscriptionId, StomperError>,
    ) {
        todo!()
    }
    fn send(
        &self,
        _: SubscriptionId,
        _: Option<SubscriptionId>,
        _: OutboundMessage,
    ) -> Result<(), StomperError> {
        todo!()
        // self.sender.send(frame).map_err(|_| StomperError {
        //     message: "Unable to send".to_owned(),
        // })
    }
}

impl Sender for AsyncStompClient {
    fn send_callback(&self, _: Option<MessageId>, _: Result<MessageId, StomperError>) {
        todo!()
    }
}

impl Client for AsyncStompClient {
    fn connect_callback(&self, result: Result<(), StomperError>) {
        if let Err(err) = result.and_then(|()| {
            self.sender
                .send(ServerFrame::Connected(ConnectedFrame::new(
                    VersionValue::new(StompVersion::V1_2),
                    None,
                    None,
                    None,
                )))
                .map_err(|_| StomperError::new("channel error"))
        }) {
            log::error!("Error accepting client connection: {:?}", err);
            if let Err(_) = self
                .sender
                .send(ServerFrame::Error(ErrorFrame::from_message(
                    err.message.as_str(),
                )))
            {
                log::error!("Error sending error: client dead?");
                todo!() // Probably need to dispose of this client.
            }
        }
    }

    fn into_sender(self: Arc<Self>) -> Arc<(dyn Sender + 'static)> {
        self
    }
    fn into_subscriber(self: Arc<Self>) -> Arc<(dyn Subscriber + 'static)> {
        self
    }
}

impl AsyncStompClient {
    pub fn create(sender: UnboundedSender<ServerFrame>) -> Self {
        AsyncStompClient { sender }
    }
}

pub struct ClientSession<T>
where
    T: Destinations + Sized,
{
    destinations: T,
    server_frame_sink: Pin<Box<dyn Sink<ServerFrame, Error = StomperError> + Send>>,
}

impl<T> ClientSession<T>
where
    T: Destinations + Sized + 'static,
{
    pub fn new(
        server_frame_sink: Pin<Box<dyn Sink<ServerFrame, Error = StomperError> + Send>>,
        destinations: T,
    ) -> ClientSession<T> {
        ClientSession {
            destinations,
            server_frame_sink: server_frame_sink,
        }
    }
    pub fn process_stream(
        self,
        stream: Pin<Box<dyn Stream<Item = Result<Vec<u8>, StomperError>> + Send>>,
    ) -> impl Future<Output = Result<(), StomperError>> + Send {
        let (tx, rx) = mpsc::unbounded_channel();

        let client = Arc::new(AsyncStompClient::create(tx));

        let handler = FrameHandlerImpl {};

        let destinations = self.destinations;

        let err_client = client.clone();
        let ready_when_done = stream
            .and_then(|bytes| async { client_frame(bytes) }.err_into())
            .and_then(move |client_frame| {
                handler.handle(client_frame, &destinations, client.clone())
            })
            .inspect_err(move |err| {
                send_error_and_close(&*err_client, &format!("Websocket error: {}", err));
            })
            .try_take_while(|cont| ready(Ok(*cont)))
            .try_fold((), |_, _| ready(Ok(())));

        tokio::task::spawn(
            UnboundedReceiverStream::new(rx)
                .take_until(ready_when_done) // Ensures this stream will end when the input stream has been processed to completion
                .map(|msg| Ok(msg))
                .forward(self.server_frame_sink)
                .unwrap_or_else(|err| {
                    info!("Error: {}", err);
                    ()
                }),
        )
        .inspect(|_| info!("Client completing"))
        .map_err(|_| StomperError::new("Unable to join response task"))
    }
}

pub fn send_error_and_close(client: &dyn StompClient, text: &str) {
    info!("Error: {}", text);
    todo!()
    // match client.send(ServerFrame::Error(ErrorFrame::from_message(text))) {
    //     Ok(_) => {}
    //     Err(some_error) => {
    //         info!("Sending error to client failed: {}", some_error);
    //     }
    // }
}

#[cfg(test)]
mod tests {
    use super::AsyncStompClient;
    use crate::client::Client;
    use stomp_parser::model::ErrorFrame;
    use stomp_parser::model::ServerFrame;
    use tokio::sync::mpsc;

    // #[tokio::test]
    // async fn it_calls_sender() {
    //     let (tx, mut rx) = mpsc::unbounded_channel();

    //     let client = AsyncStompClient::create(tx);

    //     let result = client.send(ServerFrame::Error(ErrorFrame::from_message("none")));

    //     if let Err(_) = result {
    //         panic!("Send failed");
    //     }

    //     if let Some(ServerFrame::Error(x)) = rx.recv().await {
    //         assert_eq!("none", unsafe {
    //             std::str::from_utf8_unchecked(x.body().unwrap_or(b"Foo"))
    //         })
    //     } else {
    //         panic!("No, or incorrect, message received");
    //     }
    // }

    // #[tokio::test]
    // async fn returns_error_on_failure() {
    //     let (tx, mut rx) = mpsc::unbounded_channel();

    //     let client = AsyncStompClient::create(tx);

    //     rx.close();

    //     let result = client.send(ServerFrame::Error(ErrorFrame::from_message("none")));

    //     if let Err(error) = result {
    //         assert_eq!("Unable to send", error.message)
    //     } else {
    //         panic!("No, or incorrect, error message received");
    //     }
    // }
}
