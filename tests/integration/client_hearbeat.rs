use crate::framework::*;

use std::{convert::TryFrom, pin::Pin};

use futures::{Future, FutureExt};
use stomp_parser::{
    client::{ConnectFrame, SubscribeFrame},
    headers::{
        AcceptVersionValue, DestinationValue, HeaderValue, HeartBeatIntervalls, HeartBeatValue,
        HostValue, IdValue, StompVersion, StompVersions,
    },
    server::ServerFrame,
};

#[tokio::test]
async fn connect_accepts_supplied_heartbeat() {
    test_client_expectations(connect_replies_connected).await;
}

fn connect_replies_connected(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        let connect = ConnectFrame::new(
            HostValue::new("here"),
            AcceptVersionValue::new(StompVersions(vec![StompVersion::V1_2])),
            Some(HeartBeatValue::new(HeartBeatIntervalls::new(5000, 0))),
            None,
            None,
        );

        send_data(&in_sender, connect);

        tokio::task::yield_now().await;

        assert_receive(&mut out_receiver, |bytes| {
            match ServerFrame::try_from(bytes) {
                Ok(ServerFrame::Connected(connected)) => {
                    let hb = connected.heartbeat.expect("Heartbeat not provided");
                    hb.value().supplied == 0 && hb.value().expected == 5000
                }
                _ => false,
            }
        });

        (in_sender, out_receiver)
    }
    .boxed()
}

#[tokio::test]
async fn error_after_missed_heartbeat() {
    test_client_expectations(connect_replies_connected.then(wait_for_error)).await;
}

pub fn wait_for_error(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        // nothing yet
        assert!(matches!(out_receiver.recv().now_or_never(), None));

        sleep_in_pause(6500).await;

        assert_receive(&mut out_receiver, |bytes| {
            matches!(ServerFrame::try_from(bytes), Ok(ServerFrame::Error(_)))
        });

        (in_sender, out_receiver)
    }
    .boxed()
}

#[tokio::test]
async fn disconnects_after_error() {
    test_client_expectations(
        connect_replies_connected
            .then(wait_for_error)
            .then(wait_for_disconnect),
    )
    .await;
}

#[tokio::test]
async fn connection_lingers() {
    test_client_expectations(
        connect_replies_connected
            .then(wait_for_error)
            .then(wait_and_check_alive)
            .then(wait_for_disconnect),
    )
    .await;
}

fn wait_and_check_alive(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        sleep_in_pause(100).await;

        // We did not receive a message from now_or_never; a disconnect would be Some(None)
        assert!(matches!(out_receiver.recv().now_or_never(), None));

        (in_sender, out_receiver)
    }
    .boxed()
}

#[tokio::test]
async fn heartbeat_delays_error() {
    test_client_expectations(
        connect_replies_connected
            .then(send_hearbeat)
            .then(wait_for_error),
    )
    .await;
}

#[tokio::test]
async fn frame_delays_error() {
    test_client_expectations(
        connect_replies_connected
            .then(subscribe)
            .then(wait_for_error),
    )
    .await;
}

fn subscribe(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        sleep_in_pause(5000).await;
        send_data(
            &in_sender,
            SubscribeFrame::new(
                DestinationValue::new("foo"),
                IdValue::new("MySub"),
                None,
                None,
                Vec::new(),
            ),
        );
        sleep_in_pause(2000).await;

        // No error
        assert!(matches!(out_receiver.recv().now_or_never(), None));

        (in_sender, out_receiver)
    }
    .boxed()
}

fn send_hearbeat(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        sleep_in_pause(5000).await;

        in_sender.send(Ok(b"\n".to_vec())).expect("Failed");

        sleep_in_pause(2000).await;

        // No error
        assert!(matches!(out_receiver.recv().now_or_never(), None));

        (in_sender, out_receiver)
    }
    .boxed()
}
