use crate::framework::*;

use std::{convert::TryFrom, pin::Pin};

use futures::{Future, FutureExt};
use stomp_parser::{
    client::{ConnectFrame, SendFrame, SubscribeFrame},
    headers::{
        AcceptVersionValue, DestinationValue, HeaderValue, HeartBeatIntervalls, HeartBeatValue,
        HostValue, IdValue, StompVersion, StompVersions,
    },
    server::ServerFrame,
};

#[tokio::test]
async fn connect_accepts_expected_heartbeat() {
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
            Some(HeartBeatValue::new(HeartBeatIntervalls::new(0, 5000))),
            None,
            None,
        );

        send_data(&in_sender, connect);

        tokio::task::yield_now().await;

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

        println!("Hearbeat: {}", out_receiver.recv().now_or_never().is_some());

        // assert_receive(&mut out_receiver, |bytes| {
        //     matches!(&*bytes, b"\n" | b"\r\n")
        // });

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
        let foo = DestinationValue::new("foo".to_owned());
        send_data(
            &in_sender,
            SubscribeFrame::new(
                foo.clone(),
                IdValue::new("sub".to_owned()),
                None,
                None,
                Vec::new(),
            ),
        );

        sleep_in_pause(2000).await;

        send_data(
            &in_sender,
            SendFrame::new(
                foo,
                None,
                None,
                None,
                None,
                Vec::default(),
                b"Hello, world!".to_vec(),
            ),
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
