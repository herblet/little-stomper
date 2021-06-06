mod framework;

use framework::*;

use std::{convert::TryFrom, pin::Pin, time::Duration};

use futures::{Future, FutureExt};
use stomp_parser::{
    client::ConnectFrame,
    headers::{
        AcceptVersionValue, HeartBeatIntervalls, HeartBeatValue, HostValue, StompVersion,
        StompVersions,
    },
    server::ServerFrame,
};

#[tokio::test]
async fn connect_works() {
    test_client_expectations(connect_replies_connected).await;
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

        assert_receive(&mut out_receiver, |bytes| {
            matches!(ServerFrame::try_from(bytes), Ok(ServerFrame::Connected(_)))
        });

        (in_sender, out_receiver)
    }
    .boxed()
}

#[tokio::test]
async fn server_sends_requested_heartbeat() {
    test_client_expectations(connect_replies_connected.then(expect_heartbeat)).await;
}

fn expect_heartbeat(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        tokio::time::pause();
        tokio::time::sleep(Duration::from_millis(550)).await;
        tokio::time::resume();

        assert_receive(&mut out_receiver, |bytes| {
            matches!(&*bytes, b"\n" | b"\r\n")
        });

        (in_sender, out_receiver)
    }
    .boxed()
}

#[tokio::test]
async fn server_continues_sending_heartbeat() {
    test_client_expectations(
        connect_replies_connected
            .then(expect_heartbeat)
            .then(expect_heartbeat)
            .then(expect_heartbeat)
            .then(expect_heartbeat)
            .then(expect_heartbeat),
    )
    .await;
}
