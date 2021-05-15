use crate::client::Client;
use crate::destinations::{
    DestinationId, MessageId, OutboundMessage, Sender, Subscriber, SubscriptionId,
};
use crate::error::StomperError;
use crate::Destinations;
use crate::FrameHandler;
use crate::FrameHandlerImpl;

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
        // don't really care (for now?)
        info!("Subscribed")
    }
    fn unsubscribe_callback(
        &self,
        _: Option<SubscriptionId>,
        _: std::result::Result<SubscriptionId, StomperError>,
    ) {
        //don't really care (for now?)
        info!("Unsubscribed")
    }
    fn send(
        &self,
        _: SubscriptionId,
        client_subscription_id: Option<SubscriptionId>,
        message: OutboundMessage,
    ) -> Result<(), StomperError> {
        let raw_body = message.body;

        let mut message = MessageFrame::new(
            MessageIdValue::new(message.message_id.to_string()),
            DestinationValue::new(message.destination.to_string()),
            SubscriptionValue::new(
                client_subscription_id
                    .map(|sub_id| sub_id.to_string())
                    .unwrap_or(String::from("unknown")),
            ),
            Some(ContentTypeValue::new("text/plain".to_owned())),
            Some(ContentLengthValue::new(raw_body.len() as u32)),
            (0, raw_body.len()),
        );

        message.set_raw(raw_body);

        self.sender
            .send(ServerFrame::Message(message))
            .or(Err(StomperError::new("Unable to send")))
    }
}

impl Sender for AsyncStompClient {
    fn send_callback(&self, _: Option<MessageId>, _: Result<MessageId, StomperError>) {
        //don't really care (for now?)
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
            self.error(err.message.as_str());
        }
    }

    fn into_sender(self: Arc<Self>) -> Arc<(dyn Sender + 'static)> {
        self
    }
    fn into_subscriber(self: Arc<Self>) -> Arc<(dyn Subscriber + 'static)> {
        self
    }

    fn error(&self, message: &str) {
        if let Err(_) = self
            .sender
            .send(ServerFrame::Error(ErrorFrame::from_message(message)))
        {
            log::error!("Error sending error: client dead?");
            //Probably need to dispose of this client.
        }
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
                err_client.error(&format!("Websocket error: {}", err));
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

#[cfg(test)]
mod tests {
    use super::AsyncStompClient;
    use crate::destinations::{
        DestinationId, MessageId, OutboundMessage, Subscriber, SubscriptionId,
    };
    use stomp_parser::model::ServerFrame;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn it_calls_sender() {
        let (tx, mut rx) = mpsc::unbounded_channel();

        let client = AsyncStompClient::create(tx);

        let result = client.send(
            SubscriptionId::from("Arbitrary"),
            Some(SubscriptionId::from("sub-1")),
            OutboundMessage {
                message_id: MessageId::from("1"),
                destination: DestinationId::from("somedest"),
                body: "Hello, World".as_bytes().to_owned(),
            },
        );

        if let Err(_) = result {
            panic!("Send failed");
        }

        if let Some(ServerFrame::Message(frame)) = rx.recv().await {
            assert_eq!(
                "Hello, World",
                std::str::from_utf8(frame.body().unwrap()).unwrap()
            );
        } else {
            panic!("No, or incorrect, message received");
        }
    }

    #[tokio::test]
    async fn returns_error_on_failure() {
        let (tx, mut rx) = mpsc::unbounded_channel();

        let client = AsyncStompClient::create(tx);

        rx.close();

        let result = client.send(
            SubscriptionId::from("Arbitrary"),
            Some(SubscriptionId::from("sub-1")),
            OutboundMessage {
                message_id: MessageId::from("1"),
                destination: DestinationId::from("somedest"),
                body: "Hello, World".as_bytes().to_owned(),
            },
        );

        if let Err(error) = result {
            assert_eq!("Unable to send", error.message)
        } else {
            panic!("No, or incorrect, error message received");
        }
    }
}
