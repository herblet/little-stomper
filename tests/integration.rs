use std::{convert::TryFrom, pin::Pin, sync::Arc, time::Duration};

use futures::{
    future::{join, ready},
    Future, FutureExt,
};
use little_stomper::asynchronous::{
    client::ClientSession, destinations::AsyncDestinations, inmemory::InMemDestination,
    mpsc_sink::UnboundedSenderSink,
};
use stomp_parser::{
    client::ConnectFrame,
    headers::{
        AcceptVersionValue, HeartBeatIntervalls, HeartBeatValue, HostValue, StompVersion,
        StompVersions,
    },
    server::ServerFrame,
};
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    time::sleep,
};
use tokio_stream::wrappers::UnboundedReceiverStream;

type InSender = UnboundedSender<Result<Vec<u8>, little_stomper::error::StomperError>>;
type OutReceiver = UnboundedReceiver<Vec<u8>>;
type BehaviourFunction = Box<
    dyn FnOnce(
            InSender,
            OutReceiver,
        ) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>>
        + Send,
>;

trait Chainable {
    fn then(self, followed_by: BehaviourFunction) -> BehaviourFunction;
}

impl Chainable for BehaviourFunction {
    fn then(self, followed_by: BehaviourFunction) -> BehaviourFunction {
        Box::new(|in_sender, out_receiver| {
            self(in_sender, out_receiver)
                .then(|(in_sender, out_receiver)| followed_by(in_sender, out_receiver))
                .boxed()
        })
    }
}

#[tokio::test]
async fn connect_works() {
    test_client_expectations(Box::new(connect_replies_connected)).await;
}

#[tokio::test]
async fn server_sends_requested_heartbeat() {
    let connect: BehaviourFunction = Box::new(connect_replies_connected);

    test_client_expectations(connect.then(Box::new(expect_heartbeat))).await;
}

#[tokio::test]
async fn server_continues_sending_heartbeat() {
    let connect: BehaviourFunction = Box::new(connect_replies_connected);

    test_client_expectations(
        connect
            .then(Box::new(expect_heartbeat))
            .then(Box::new(expect_heartbeat))
            .then(Box::new(expect_heartbeat))
            .then(Box::new(expect_heartbeat))
            .then(Box::new(expect_heartbeat)),
    )
    .await;
}

async fn test_client_expectations(client_behaviour: BehaviourFunction) {
    let (in_sender, in_receiver) = unbounded_channel();
    let (out_sender, out_receiver) = unbounded_channel();

    let destinations = AsyncDestinations::start(Arc::new(InMemDestination::create)).await;

    let session_future = ClientSession::process_stream(
        Box::pin(UnboundedReceiverStream::new(in_receiver)),
        Box::pin(UnboundedSenderSink::from(out_sender)),
        destinations,
    )
    .boxed();

    let client_behaviour = client_behaviour(in_sender, out_receiver);

    let other_future = tokio::task::spawn(client_behaviour.then(|(in_sender, out_receiver)| {
        let mut receiver = out_receiver;
        receiver.close();
        drop(in_sender);
        ready(())
    }));

    let results = join(session_future, other_future).await;

    assert!(results.0.is_ok());
    assert!(results.1.is_ok());
}

fn connect_replies_connected(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        let connect = ConnectFrame::new(
            HostValue::new("here".to_owned()),
            AcceptVersionValue::new(StompVersions(vec![StompVersion::V1_2])),
            Some(HeartBeatValue::new(HeartBeatIntervalls::new(0, 500))),
            None,
            None,
        );

        in_sender
            .send(Ok(connect.to_string().into_bytes()))
            .expect("Connect failed");

        tokio::task::yield_now().await;

        let response = out_receiver.recv().now_or_never();

        if let Some(Some(bytes)) = response {
            let frame = ServerFrame::try_from(bytes);

            assert!(matches!(frame, Ok(ServerFrame::Connected(_))))
        } else {
            panic!("Unexpected server message");
        }
        (in_sender, out_receiver)
    }
    .boxed()
}

fn expect_heartbeat(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        tokio::time::pause();
        tokio::time::sleep(Duration::from_millis(550)).await;
        tokio::time::resume();

        let response = out_receiver.recv().now_or_never();

        if let Some(Some(bytes)) = response {
            assert!(matches!(&*bytes, b"\n" | b"\r\n"));
        } else {
            if response.is_none() {
                panic!("No server message");
            } else {
                panic!("Unexpected server message:{:?}", response.unwrap());
            }
        }

        (in_sender, out_receiver)
    }
    .boxed()
}
