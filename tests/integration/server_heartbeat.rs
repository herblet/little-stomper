use crate::framework::*;

use std::{convert::TryFrom, pin::Pin};

use futures::{Future, FutureExt};
use stomp_parser::{
    client::{ConnectFrameBuilder, SendFrameBuilder, SubscribeFrameBuilder},
    headers::{HeartBeatIntervalls, StompVersion, StompVersions},
    server::ServerFrame,
};
use tokio::task::yield_now;

#[tokio::test]
async fn connect_accepts_expected_heartbeat() {
    test_client_expectations(connect_replies_connected).await;
}

fn connect_replies_connected(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        let connect = ConnectFrameBuilder::new()
            .host("here".to_owned())
            .accept_version(StompVersions(vec![StompVersion::V1_2]))
            .heartbeat(HeartBeatIntervalls::new(0, 5000))
            .build()
            .unwrap();

        send_data(&in_sender, connect);

        yield_now().await;

        assert_receive(&mut out_receiver, |bytes| {
            match ServerFrame::try_from(bytes) {
                Ok(ServerFrame::Connected(connected)) => {
                    let hb = connected.heartbeat.expect("Heartbeat not provided");
                    hb.value().expected == 0 && hb.value().supplied == 5000
                }
                _ => false,
            }
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
        sleep_in_pause(5050).await;

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

#[tokio::test]
async fn server_sends_message_to_subscriber() {
    test_client_expectations(connect_replies_connected.then(subscribe_send_receive)).await;
}

fn subscribe_send_receive(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        let foo = "foo".to_owned();

        send_data(
            &in_sender,
            SubscribeFrameBuilder::new()
                .destination(foo.clone())
                .id("sub".to_owned())
                .build()
                .unwrap(),
        );

        sleep_in_pause(2000).await;

        send_data(
            &in_sender,
            SendFrameBuilder::new()
                .destination(foo)
                .body(b"Hello, world!".to_vec())
                .build()
                .unwrap(),
        );

        sleep_in_pause(1000).await;

        assert_receive(&mut out_receiver, |bytes| {
            if let Ok(ServerFrame::Message(frame)) = ServerFrame::try_from(bytes) {
                "Hello, world!" == String::from_utf8(frame.body().unwrap().to_vec()).unwrap()
            } else {
                false
            }
        });

        (in_sender, out_receiver)
    }
    .boxed()
}

#[tokio::test]
async fn server_message_delays_heartbeat() {
    test_client_expectations(
        connect_replies_connected
            .then(subscribe_send_receive)
            .then(delayed_heartbeat),
    )
    .await;
}

fn delayed_heartbeat(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        sleep_in_pause(3000).await;

        if let Some(_) = out_receiver.recv().now_or_never() {
            panic!("Heartbeat should be delayed")
        }

        sleep_in_pause(2000).await;

        assert_receive(&mut out_receiver, |bytes| {
            matches!(&*bytes, b"\n" | b"\r\n")
        });

        (in_sender, out_receiver)
    }
    .boxed()
}
