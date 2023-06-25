use stomp_test_utils::*;

use std::{convert::TryFrom, pin::Pin};

use futures::{Future, FutureExt};
use stomp_parser::{
    client::{ConnectFrameBuilder, SendFrameBuilder, SubscribeFrameBuilder},
    headers::{HeartBeatIntervalls, StompVersion, StompVersions},
    server::ServerFrame,
};
use tokio::task::yield_now;

use super::*;

#[tokio::test]
async fn connect_accepts_expected_heartbeat() {
    assert_client_behaviour(connect_replies_connected).await;
}

fn connect_replies_connected<'a>(
    in_sender: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    async move {
        let connect =
            ConnectFrameBuilder::new("here".to_owned(), StompVersions(vec![StompVersion::V1_2]))
                .heartbeat(HeartBeatIntervalls::new(0, 5000))
                .build();

        send_data(in_sender, connect);

        yield_now().await;

        assert_receive(out_receiver, |bytes| match ServerFrame::try_from(bytes) {
            Ok(ServerFrame::Connected(connected)) => {
                let hb = connected.heartbeat.expect("Heartbeat not provided");
                hb.value().expected == 0 && hb.value().supplied == 5000
            }
            _ => false,
        });

        
    }
    .boxed()
}

#[tokio::test]
async fn server_sends_requested_heartbeat() {
    assert_client_behaviour(connect_replies_connected.then(expect_heartbeat)).await;
}

fn expect_heartbeat<'a>(
    _: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    async move {
        sleep_in_pause(5050).await;

        assert_receive(out_receiver, |bytes| matches!(&*bytes, b"\n" | b"\r\n"));

        
    }
    .boxed()
}

#[tokio::test]
async fn server_continues_sending_heartbeat() {
    assert_client_behaviour(
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
    assert_client_behaviour(connect_replies_connected.then(subscribe_send_receive)).await;
}

fn subscribe_send_receive<'a>(
    in_sender: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    async move {
        let foo = "foo".to_owned();

        send_data(
            in_sender,
            SubscribeFrameBuilder::new(foo.clone(), "sub".to_owned()).build(),
        );

        sleep_in_pause(2000).await;

        send_data(
            in_sender,
            SendFrameBuilder::new(foo)
                .body(b"Hello, world!".to_vec())
                .build(),
        );

        sleep_in_pause(1000).await;

        assert_receive(out_receiver, |bytes| {
            if let Ok(ServerFrame::Message(frame)) = ServerFrame::try_from(bytes) {
                "Hello, world!" == String::from_utf8(frame.body().unwrap().to_vec()).unwrap()
            } else {
                false
            }
        });

        
    }
    .boxed()
}

#[tokio::test]
async fn server_message_delays_heartbeat() {
    assert_client_behaviour(
        connect_replies_connected
            .then(subscribe_send_receive)
            .then(delayed_heartbeat),
    )
    .await;
}

fn delayed_heartbeat<'a>(
    _: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    async move {
        sleep_in_pause(3000).await;

        if out_receiver.recv().now_or_never().is_some() {
            panic!("Heartbeat should be delayed")
        }

        sleep_in_pause(2000).await;

        assert_receive(out_receiver, |bytes| matches!(&*bytes, b"\n" | b"\r\n"));

        
    }
    .boxed()
}
