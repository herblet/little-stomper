// pub mod asynchronous;
// // Must come before modules using the macros!
// mod macros;

// mod client;
// mod destinations;
// mod error;
// mod frame_handler;

use little_stomper::asynchronous::client::ClientSession;
use little_stomper::asynchronous::destinations::{AsyncDestinations, DestinationType};
use little_stomper::asynchronous::inmemory::InMemDestination;
use little_stomper::client::{DefaultClient, DefaultClientFactory};
use little_stomper::destinations::Destinations;
use little_stomper::error::StomperError;

use sender_sink::wrappers::UnboundedSenderSink;

// use futures_util::StreamExt;
use futures::future::{FutureExt, TryFutureExt};
use futures::sink::SinkExt;
use futures::stream::{StreamExt, TryStreamExt};
use std::future::ready;

use log::info;
use std::future::Future;

use std::sync::Arc;
use std::{env, io::Error};
use tokio_stream::wrappers::UnboundedReceiverStream;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

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
                info!("Client connection dropped");

                // Need to shut the whole shebang down - we'll forward the message to achieve that
                if self.message_channel.send(msg).is_err() {
                    info!("Trying to close channel; alread closed");
                };

                return Err(StomperError {
                    message: "WebSocketChannel closed".to_owned(),
                });
            }
            Message::Text(text) => Some(text.into_bytes()),

            Message::Binary(bytes) => Some(bytes),

            Message::Frame(_) => {
                // Not sure what to do, ignore for now
                None
            }
        })
    }
}

pub fn handle_websocket<T: Destinations<Client = DefaultClient> + 'static>(
    websocket: WebSocketStream<tokio::net::TcpStream>,
    destinations: T,
) -> impl Future<Output = Result<(), StomperError>> + Send {
    let (socket_tx, ws_stream) = websocket.split();

    let (ws_tx, ws_rx) = mpsc::unbounded_channel();

    let pong_channel = ws_tx.clone();
    //let ping_channel = ws_tx.clone();

    //ping_channel.send(Message::Ping(vec![])).unwrap();

    // tokio::spawn(async move {
    //     let mut interval = time::interval(Duration::from_secs(1));
    //     loop {
    //         interval.tick().await;
    //         if let Err(e) = ping_channel.send(Message::Ping(vec![])) {
    //             info!("Error sending ping: {}", e);
    //         }
    //     }
    // });

    let ws_response_processor_handle = tokio::task::spawn(async move {
        UnboundedReceiverStream::new(ws_rx)
            // Stop when a close message is received
            .take_while(|msg: &Message| ready(!msg.is_close()))
            .map(Ok)
            .forward(socket_tx)
            .unwrap_or_else(|err| {
                info!("Error: {}", err);
            })
            .await;
    });

    // Transform the stream of websocket messages into a stream of byte arrays for the Stomp part;
    // handle non-Stomp message types along the way
    let bytes_stream = ws_stream
        .map_err(|err| StomperError {
            message: format!("Websocket error: {}", err.to_string()),
        })
        .inspect_err(|err| log::error!("Error: {:?}", err))
        .try_filter_map(WebSocketMessageHandler::new(pong_channel).into_filter_map())
        .boxed();

    let sink = UnboundedSenderSink::from(ws_tx)
        .with(|bytes: Vec<u8>| ready(Ok(Message::text(String::from_utf8(bytes).unwrap()))));

    ClientSession::process_stream(
        bytes_stream,
        Box::pin(sink),
        destinations,
        DefaultClientFactory {},
    )
    .then(move |_| async {
        ws_response_processor_handle
            .await
            .map_err(|_| StomperError {
                message: "Error awaiting WebSocket response handler".to_owned(),
            })
    })
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

    if let Err(err) = handle_websocket(ws_stream, destinations).await {
        log::error!("Error handling websocket: {}", err);
    }

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
