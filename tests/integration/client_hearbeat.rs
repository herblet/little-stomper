use little_stomper::error::StomperError;
use stomp_test_utils::*;

use std::{convert::TryFrom, pin::Pin};

use futures::{Future, FutureExt};

use stomp_parser::{
    client::{ConnectFrameBuilder, SubscribeFrameBuilder},
    headers::{HeartBeatIntervalls, StompVersion, StompVersions},
    server::ServerFrame,
};

use super::*;

#[tokio::test]
async fn connect_accepts_supplied_heartbeat() {
    assert_client_behaviour(connect_replies_connected).await;
}

fn connect_replies_connected<'a>(
    in_sender: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    send(
        ConnectFrameBuilder::new("here".to_owned(), StompVersions(vec![StompVersion::V1_2]))
            .heartbeat(HeartBeatIntervalls::new(5000, 0))
            .build(),
    )
    .then(receive(|bytes| match ServerFrame::try_from(bytes) {
        Ok(ServerFrame::Connected(connected)) => {
            let hb = connected.heartbeat.expect("Heartbeat not provided");
            hb.value().supplied == 0 && hb.value().expected == 5000
        }
        _ => false,
    }))(in_sender, out_receiver)
}

#[tokio::test]
async fn error_after_missed_heartbeat() {
    let a = connect_replies_connected.then(wait_for_error);
    assert_client_behaviour(a).await;
}

pub fn wait_for_error<'a>(
    _: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    async move {
        // nothing yet
        assert!(matches!(out_receiver.recv().now_or_never(), None));

        sleep_in_pause(6500).await;

        assert_receive(out_receiver, |bytes| {
            matches!(ServerFrame::try_from(bytes), Ok(ServerFrame::Error(_)))
        });
    }
    .boxed()
}

#[tokio::test]
async fn disconnects_after_error() {
    let a = connect_replies_connected
        .then(wait_for_error)
        .then(wait_for_disconnect);

    assert_client_behaviour(a).await;
}

#[tokio::test]
async fn connection_lingers() {
    assert_client_behaviour(
        connect_replies_connected
            .then(wait_for_error)
            .then(wait_and_check_alive)
            .then(wait_for_disconnect),
    )
    .await;
}

fn wait_and_check_alive<'a>(
    _: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    async move {
        sleep_in_pause(100).await;

        // We did not receive a message from now_or_never; a disconnect would be Some(None)
        assert!(matches!(out_receiver.recv().now_or_never(), None));
    }
    .boxed()
}

#[tokio::test]
async fn heartbeat_delays_error() {
    assert_client_behaviour(
        connect_replies_connected
            .then(send_hearbeat)
            .then(wait_for_error),
    )
    .await;
}

#[tokio::test]
async fn frame_delays_error() {
    assert_client_behaviour(
        connect_replies_connected
            .then(subscribe)
            .then(wait_for_error),
    )
    .await;
}

fn subscribe<'a>(
    in_sender: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    async move {
        sleep_in_pause(5000).await;
        send_data(
            in_sender,
            SubscribeFrameBuilder::new("foo".to_owned(), "MySub".to_owned()).build(),
        );
        sleep_in_pause(2000).await;

        // No error
        assert!(matches!(out_receiver.recv().now_or_never(), None));
    }
    .boxed()
}

fn send_hearbeat<'a>(
    in_sender: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    async move {
        sleep_in_pause(5000).await;

        in_sender.send(Ok(b"\n".to_vec())).expect("Failed");

        sleep_in_pause(2000).await;

        // No error
        assert!(matches!(out_receiver.recv().now_or_never(), None));
    }
    .boxed()
}
