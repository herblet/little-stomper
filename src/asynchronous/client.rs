use crate::asynchronous::delayable_stream::ResettableTimer;
use crate::client::Client;
use crate::destinations::{
    DestinationId, Destinations, InboundMessage, MessageId, OutboundMessage, Sender, Subscriber,
    SubscriptionId,
};
use crate::error::StomperError;

use either::Either;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::{FutureExt, StreamExt, TryFutureExt, TryStreamExt};

use futures::stream::select_all::select_all;
use log::info;
use std::convert::TryFrom;
use std::future::ready;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use stomp_parser::client::*;
use stomp_parser::headers::*;
use stomp_parser::server::*;
use tokio_stream::wrappers::UnboundedReceiverStream;

use tokio::sync::mpsc::{self, UnboundedSender};

use std::collections::HashMap;

use super::delayable_stream::ResettableTimerResetter;

const EOL: &'static [u8; 1] = b"\n";

/// Indicates or changes the current state of the client
enum ClientState {
    Alive,
    Dead,
}

/// And Event which a AsyncStompClient can receive
enum ClientEvent {
    /// A Frame from the client if received and parsed correctly, or Err if there was an error
    ClientFrame(Result<ClientFrame, StomperError>),

    /// A Message the server wishes to send to the client, specifying the (client's) subscription id, as well as the message itself
    ServerMessage(SubscriptionId, OutboundMessage),

    /// A callback indicating the result of an attempt to subscribe the client to a destination
    Subscribed(
        DestinationId,
        SubscriptionId,
        Result<SubscriptionId, StomperError>,
    ),

    /// A callback indicating the result of an attempt to unsubscribe the client from a destination
    Unsubscribed(SubscriptionId, Result<SubscriptionId, StomperError>),
    /// A callback indicating the result of an attempt to connect
    Connected(Result<(), StomperError>),

    /// An error that should be communicated to the client
    Error(String),

    /// Send a heartbeat to the client
    Heartbeat,

    /// An event indicating the client connection was closed.
    Close,
}

#[derive(Debug)]
pub struct AsyncStompClient {
    sender: UnboundedSender<ClientEvent>,
}

impl AsyncStompClient {
    fn send_event(&self, event: ClientEvent) {
        if let Err(_) = self.sender.send(event) {
            info!("Unable to send ClientEvent, channel closed?");
        }
    }

    fn unwrap_subscriber_sub_id(subscriber_sub_id: Option<SubscriptionId>) -> SubscriptionId {
        subscriber_sub_id
            .expect("STOMP requires subscriptions to have a client-provided identifier")
    }
}

impl Subscriber for AsyncStompClient {
    fn subscribe_callback(
        &self,
        destination_id: DestinationId,
        client_subscription_id: Option<SubscriptionId>,
        subscribe_result: Result<SubscriptionId, StomperError>,
    ) {
        self.send_event(ClientEvent::Subscribed(
            destination_id,
            AsyncStompClient::unwrap_subscriber_sub_id(client_subscription_id),
            subscribe_result,
        ));
    }
    fn unsubscribe_callback(
        &self,
        client_subscription_id: Option<SubscriptionId>,
        unsubscribe_result: std::result::Result<SubscriptionId, StomperError>,
    ) {
        self.send_event(ClientEvent::Unsubscribed(
            AsyncStompClient::unwrap_subscriber_sub_id(client_subscription_id),
            unsubscribe_result,
        ));
    }

    fn send(
        &self,
        _: SubscriptionId,
        client_subscription_id: Option<SubscriptionId>,
        message: OutboundMessage,
    ) -> Result<(), StomperError> {
        self.sender
            .send(ClientEvent::ServerMessage(
                AsyncStompClient::unwrap_subscriber_sub_id(client_subscription_id),
                message,
            ))
            .map_err(|_| StomperError::new("Unable to send message, client channel closed"))
    }
}

impl Sender for AsyncStompClient {
    fn send_callback(&self, _: Option<MessageId>, _: Result<MessageId, StomperError>) {
        //don't really care (for now?)
    }
}

impl Client for AsyncStompClient {
    fn connect_callback(&self, result: Result<(), StomperError>) {
        self.send_event(ClientEvent::Connected(result));
    }

    fn into_sender(self: Arc<Self>) -> Arc<(dyn Sender + 'static)> {
        self
    }
    fn into_subscriber(self: Arc<Self>) -> Arc<(dyn Subscriber + 'static)> {
        self
    }

    fn error(&self, message: &str) {
        self.send_event(ClientEvent::Error(message.to_owned()));
    }

    fn send_heartbeat(&self) {
        self.send_event(ClientEvent::Heartbeat)
    }
}

impl AsyncStompClient {
    fn create(sender: UnboundedSender<ClientEvent>) -> Self {
        AsyncStompClient { sender }
    }
}

type ResultType = Pin<
    Box<
        dyn Future<
                Output = Result<(ClientState, Option<Either<ServerFrame, Vec<u8>>>), StomperError>,
            > + Send
            + 'static,
    >,
>;

fn frame_result(frame: ServerFrame) -> ResultType {
    ready(Ok((ClientState::Alive, Some(Either::Left(frame))))).boxed()
}
pub struct ClientSession<T>
where
    T: Destinations + 'static,
{
    destinations: T,
    client: Arc<AsyncStompClient>,
    active_subscriptions_by_client_id: HashMap<SubscriptionId, (DestinationId, SubscriptionId)>,
    heartbeat_resetter: ResettableTimerResetter,
}

impl<T> Drop for ClientSession<T>
where
    T: Destinations + 'static,
{
    fn drop(&mut self) {
        self.end_heartbeat();
    }
}

impl<T> ClientSession<T>
where
    T: Destinations + 'static,
{
    fn new(
        destinations: T,
        client: Arc<AsyncStompClient>,
        heartbeat_resetter: ResettableTimerResetter,
    ) -> ClientSession<T> {
        ClientSession {
            destinations,
            client,
            active_subscriptions_by_client_id: HashMap::new(),
            heartbeat_resetter,
        }
    }

    fn unsubscribe(&mut self, client_subscription_id: SubscriptionId) -> ResultType {
        match self
            .active_subscriptions_by_client_id
            .get(&client_subscription_id)
        {
            None => self.error(&format!(
                "Attempt to unsubscribe from unknown subscription: {}",
                client_subscription_id
            )),
            Some((destination_id, destination_sub_id)) => {
                self.destinations.unsubscribe(
                    destination_id.clone(),
                    destination_sub_id.clone(),
                    self.client.clone().into_subscriber(),
                );
                ready(Ok((ClientState::Alive, None))).boxed()
            }
        }
    }

    fn client_frame(&mut self, frame: Result<ClientFrame, StomperError>) -> ResultType {
        match frame {
            Err(err) => self.error(&format!("Error processing client message: {:?}", err)),
            Ok(frame) => self.handle(frame).boxed(),
        }
    }

    fn subscribed(
        &mut self,
        destination: DestinationId,
        client_subscription_id: SubscriptionId,
        result: Result<SubscriptionId, StomperError>,
    ) -> ResultType {
        if let Ok(destination_sub_id) = result {
            self.active_subscriptions_by_client_id
                .insert(client_subscription_id, (destination, destination_sub_id));
        }
        ready(Ok((ClientState::Alive, None))).boxed()
    }

    fn unsubscribed(
        &mut self,
        client_subscription_id: SubscriptionId,
        result: Result<SubscriptionId, StomperError>,
    ) -> ResultType {
        if let Ok(_) = result {
            self.active_subscriptions_by_client_id
                .remove(&client_subscription_id);
        }
        ready(Ok((ClientState::Alive, None))).boxed()
    }

    fn server_message(
        &mut self,
        client_subscription_id: SubscriptionId,
        message: OutboundMessage,
    ) -> ResultType {
        let raw_body = message.body;

        let message = MessageFrame::new(
            MessageIdValue::new(message.message_id.to_string()),
            DestinationValue::new(message.destination.to_string()),
            SubscriptionValue::new(client_subscription_id.to_string()),
            Some(ContentTypeValue::new("text/plain".to_owned())),
            Some(ContentLengthValue::new(raw_body.len() as u32)),
            raw_body,
        );

        frame_result(ServerFrame::Message(message))
    }

    fn connected(&mut self, result: Result<(), StomperError>) -> ResultType {
        match result {
            Ok(_) => frame_result(ServerFrame::Connected(ConnectedFrame::new(
                VersionValue::new(StompVersion::V1_2),
                None,
                None,
                None,
            ))),
            Err(_) => {
                log::error!("Unable to initialise session.");
                self.error("Unable to initialise session, connect failed")
                    .map_ok(|(_, frame)| (ClientState::Dead, frame))
                    .boxed()
            }
        }
    }

    fn error(&mut self, message: &str) -> ResultType {
        frame_result(ServerFrame::Error(ErrorFrame::from_message(message)))
    }

    fn send_heartbeat(&self) -> ResultType {
        println!("Sending heartbeat");
        ready(Ok((ClientState::Alive, Some(Either::Right(EOL.to_vec()))))).boxed()
    }

    fn handle_event(&mut self, event: ClientEvent) -> ResultType {
        match event {
            ClientEvent::Close => ready(Ok((ClientState::Dead, None))).boxed(),
            ClientEvent::ClientFrame(result) => self.client_frame(result),
            ClientEvent::ServerMessage(client_subscription_id, message) => {
                self.heartbeat_resetter.reset().expect("Unexpected error");
                self.server_message(client_subscription_id, message).boxed()
            }
            ClientEvent::Subscribed(destination, client_subscription_id, result) => {
                self.subscribed(destination, client_subscription_id, result)
            }
            ClientEvent::Unsubscribed(client_subscription_id, result) => {
                self.unsubscribed(client_subscription_id, result)
            }
            ClientEvent::Connected(result) => self.connected(result),
            ClientEvent::Error(message) => self.error(&message),
            ClientEvent::Heartbeat => self.send_heartbeat(),
        }
        .boxed()
    }

    async fn parse_client_message(bytes: Vec<u8>) -> Result<ClientFrame, StomperError> {
        ClientFrame::try_from(bytes).map_err(|err| err.into())
    }

    fn log_error(error: &StomperError) {
        log::error!("Error handling event: {}", error);
    }

    fn not_dead<Q>(result: &Result<(ClientState, Q), StomperError>) -> impl Future<Output = bool> {
        ready(!matches!(result, Ok((ClientState::Dead, _))))
    }

    fn into_opt_ok_of_bytes(
        result: Result<(ClientState, Option<Either<ServerFrame, Vec<u8>>>), StomperError>,
    ) -> impl Future<Output = Option<Result<Vec<u8>, StomperError>>> {
        ready(
            // Drop the ClientState, already handled
            result
                .map(|(_, opt_frame)| {
                    // serialize the frame
                    opt_frame.map(|either| match either {
                        Either::Left(frame) => frame.to_string().into_bytes(),
                        Either::Right(bytes) => bytes,
                    })
                })
                // drop errors
                .or(Ok(None))
                // cause only Some(Ok(bytes)) values to be passed on
                .transpose(),
        )
    }
    pub fn process_stream(
        stream: Pin<Box<dyn Stream<Item = Result<Vec<u8>, StomperError>> + Send + 'static>>,
        server_frame_sink: Pin<
            Box<dyn Sink<Vec<u8>, Error = StomperError> + Sync + Send + 'static>,
        >,
        destinations: T,
    ) -> impl Future<Output = Result<(), StomperError>> + Send + 'static {
        let (tx, rx) = mpsc::unbounded_channel();

        let client = Arc::new(AsyncStompClient::create(tx));

        // Closes this session; will be chained to client stream to run after that ends
        let close_stream = futures::stream::once(async { ClientEvent::Close });

        let stream_from_client = stream
            .and_then(Self::parse_client_message)
            .map(ClientEvent::ClientFrame)
            .chain(close_stream);

        // This is the stream of events generated by processing on the server-side, rather than directly from the client
        let stream_from_server = UnboundedReceiverStream::new(rx);

        let (heartbeat_stream, heartbeat_resetter) = ResettableTimer::default();

        let all_events = select_all(vec![
            stream_from_client.boxed(),
            stream_from_server.boxed(),
            heartbeat_stream.map(|_| ClientEvent::Heartbeat).boxed(),
        ]);

        let event_handler = {
            let mut client_session = ClientSession::new(destinations, client, heartbeat_resetter);

            move |event| client_session.handle_event(event)
        };

        let all_events = all_events
            .then(event_handler)
            .inspect_err(Self::log_error)
            .take_while(Self::not_dead)
            .filter_map(Self::into_opt_ok_of_bytes);

        tokio::task::spawn(all_events.forward(server_frame_sink))
            .inspect(|_| info!("Client completing"))
            .map_ok(|_| ()) // ignore the result from the forward.
            .map_err(|_| StomperError::new("Unable to join response task"))
    }

    fn start_heartbeat(&mut self, millis: u32) {
        if let Err(err) = self
            .heartbeat_resetter
            .change_period(Duration::from_millis(millis as u64))
        {
            log::error!("Error starting heartbeat: {}", err);
        }
    }

    fn end_heartbeat(&mut self) {
        // if let Some(heart_beat_task) = self.heartbeat_task.take() {
        //     heart_beat_task.abort();
        // }
    }

    fn handle(&mut self, frame: ClientFrame) -> ResultType {
        match frame {
            ClientFrame::Connect(frame) => {
                if frame.accept_version.value().contains(&StompVersion::V1_2) {
                    self.client.connect_callback(Ok(()));
                } else {
                    self.client.connect_callback(Err(StomperError::new(
                        format!("Unavailable Version {:?} requested.", frame.accept_version)
                            .as_str(),
                    )));
                }

                if frame.heartbeat.value().expected > 0 {
                    self.start_heartbeat(frame.heartbeat.value().expected);
                }
                ready(Ok((ClientState::Alive, None))).boxed()
            }

            ClientFrame::Subscribe(frame) => {
                self.destinations.subscribe(
                    DestinationId(frame.destination.value().clone()),
                    Some(SubscriptionId::from(frame.id.value())),
                    self.client.clone().into_subscriber(),
                );
                ready(Ok((ClientState::Alive, None))).boxed()
            }

            ClientFrame::Send(frame) => {
                self.destinations.send(
                    DestinationId(frame.destination.value().clone()),
                    InboundMessage {
                        sender_message_id: None,
                        body: frame.body().unwrap().to_owned(),
                    },
                    self.client.clone().into_sender(),
                );
                ready(Ok((ClientState::Alive, None))).boxed()
            }

            ClientFrame::Disconnect(_frame) => {
                info!("Client Disconnecting");
                ready(Ok((ClientState::Dead, None))).boxed()
            }
            ClientFrame::Unsubscribe(frame) => {
                self.unsubscribe(SubscriptionId(frame.id.value().clone()))
            }

            ClientFrame::Abort(_frame) => {
                todo!()
            }

            ClientFrame::Ack(_frame) => {
                todo!()
            }

            ClientFrame::Begin(_frame) => {
                todo!()
            }

            ClientFrame::Commit(_frame) => {
                todo!()
            }

            ClientFrame::Nack(_frame) => {
                todo!()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AsyncStompClient, ClientEvent};
    use crate::destinations::{
        DestinationId, MessageId, OutboundMessage, Subscriber, SubscriptionId,
    };
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

        if let Some(ClientEvent::ServerMessage(_, message)) = rx.recv().await {
            assert_eq!("Hello, World", std::str::from_utf8(&message.body).unwrap());
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
            assert_eq!(
                "Unable to send message, client channel closed",
                error.message
            )
        } else {
            panic!("No, or incorrect, error message received");
        }
    }
}
