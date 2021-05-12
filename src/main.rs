#![crate_name = "stomper"]
#![feature(trait_alias)]
extern crate log;
pub mod asynchronous;
mod client;
mod destinations;
mod error;
mod frame_handler;
mod websocket;

use asynchronous::client::AsyncStompClient;
use asynchronous::client::ClientSession;
use asynchronous::destinations::{AsyncDestinations, DestinationType, InMemDestination};
use asynchronous::mpsc_sink::UnboundedSenderSink;

use client::Client as StompClient;
use destinations::{Destination, DestinationAction, Destinations};
use error::StomperError;

use core::pin::Pin;
use std::fmt::Display;
// use futures_util::StreamExt;
use futures::future::{FutureExt, TryFutureExt};
use futures::sink::{Sink, SinkExt};
use futures::stream::{StreamExt, TryStreamExt};

use std::future::ready;

use log::info;
use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::str;
use std::sync::Arc;
use std::{env, io::Error};
use tokio_stream::wrappers::UnboundedReceiverStream;

use env_logger;
use stomp_parser::model::*;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, error::SendError, UnboundedReceiver, UnboundedSender};
use tokio::sync::RwLock;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use frame_handler::{FrameHandler, FrameHandlerImpl};

struct WebSocketHandler<T: Destinations> {
    destinations: T,
    client: Arc<AsyncStompClient>,
}

struct WebSocketMessageHandler {
    // where to call back to
    message_channel: UnboundedSender<Message>,
}

impl WebSocketMessageHandler {
    pub fn new(message_channel: UnboundedSender<Message>) -> WebSocketMessageHandler {
        WebSocketMessageHandler { message_channel }
    }

    pub fn into_filter_map(
        self,
    ) -> impl Fn(Message) -> std::future::Ready<Result<Option<Vec<u8>>, StomperError>> {
        move |msg| ready(self.handle_message(msg))
    }

    fn handle_message(&self, msg: Message) -> Result<Option<Vec<u8>>, StomperError> {
        Ok(match msg {
            Message::Ping(bytes) => {
                self.message_channel
                    .send(Message::Pong(bytes))
                    .map_err(|_| StomperError {
                        message: "Unable to send return message".to_owned(),
                    })?;
                None
            }
            Message::Pong(_) => {
                // Not sure what to do, ignore for now
                None
            }
            Message::Close(_) => {
                // Need to shut the whole shebang down - we'll forward the message to achieve that
                if let Err(_) = self.message_channel.send(msg) {
                    info!("Trying to close channel; alread closed");
                };
                return Err(StomperError {
                    message: "WebSocketChannel closed".to_owned(),
                });
            }
            Message::Text(text) => Some(text.into_bytes()),

            Message::Binary(bytes) => Some(bytes),
        })
    }
}
impl<T: Destinations + 'static> WebSocketHandler<T> {
    pub fn handle(
        websocket: WebSocketStream<tokio::net::TcpStream>,
        destinations: T,
    ) -> impl Future<Output = Result<(), StomperError>> + Send {
        let (socket_tx, ws_stream) = websocket.split();

        let (ws_tx, ws_rx) = mpsc::unbounded_channel();

        let pong_channel = ws_tx.clone();

        let ws_response_processor_handle = tokio::task::spawn(async move {
            UnboundedReceiverStream::new(ws_rx)
                // Stop when a close message is received
                .take_while(|msg| {
                    ready(if let Message::Close(_) = msg {
                        false
                    } else {
                        true
                    })
                })
                .map(|msg| Ok(msg))
                .forward(socket_tx)
                .unwrap_or_else(|err| {
                    info!("Error: {}", err);
                    ()
                })
                .await;
        });

        // Transform the stream of websocket messages into a stream of byte arrays for the Stomp part;
        // handle non-Stomp message types along the way
        let bytes_stream = ws_stream
            .map_err(|err| StomperError {
                message: format!("Websocket error: {}", err.to_string()),
            })
            .try_filter_map(WebSocketMessageHandler::new(pong_channel).into_filter_map())
            .boxed();

        let server_frame_sink = Box::pin(
            UnboundedSenderSink::from(ws_tx)
                .with(|frame| async { Ok(WebSocketHandler::<T>::to_message(frame)) }),
        );

        let session = ClientSession::new(server_frame_sink, destinations);

        session.process_stream(bytes_stream).then(move |_| async {
            ws_response_processor_handle
                .await
                .map_err(|_| StomperError {
                    message: "Error awaiting WebSocket response handler".to_owned(),
                })
        })
    }

    fn to_message(frame: ServerFrame) -> Message {
        match frame {
            ServerFrame::Connected(frame) => Message::text(frame.to_string()),
            ServerFrame::Message(frame) => Message::text(frame.to_string()),
            ServerFrame::Receipt(frame) => Message::text(frame.to_string()),
            ServerFrame::Error(frame) => Message::text(frame.to_string()),
            _ => Message::text("Error\nUnknown Frame\n\n\x00"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let _ = env_logger::try_init();

    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:3030".to_string());

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");

    info!("Listening on: {}", addr);

    let destinations =
        AsyncDestinations::<InMemDestination>::start(Arc::new(InMemDestination::create)).await;

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream, destinations.clone()));
    }

    Ok(())
}

async fn accept_connection<D: DestinationType + 'static>(
    stream: TcpStream,
    destinations: AsyncDestinations<D>,
) {
    let addr = stream
        .peer_addr()
        .expect("connected streams should have a peer address");
    info!("Peer address: {}", addr);

    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");

    info!("New WebSocket connection: {}", addr);

    WebSocketHandler::handle(ws_stream, destinations).await;

    info!(" Connection Ended: {}", addr);
}

#[cfg(test)]
mod test {
    use super::WebSocketMessageHandler;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Message;

    #[tokio::test]
    async fn message_hander_handles_ping() {
        let (tx, mut rx) = mpsc::unbounded_channel();

        let filter_map = WebSocketMessageHandler::new(tx).into_filter_map();
        filter_map(Message::Ping(b"0".to_vec())).await.unwrap();

        let msg = rx.recv().await;

        if let Some(Message::Pong(bytes)) = msg {
            assert_eq!(b"0".to_vec(), bytes);
        } else {
            panic!("Ping did not Pong!");
        }
    }
}
