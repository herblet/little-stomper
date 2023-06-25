use stomp_test_utils::*;

use std::{convert::TryFrom, pin::Pin};

use futures::{Future, FutureExt};
use stomp_parser::{
    client::{ConnectFrameBuilder, SubscribeFrame, SubscribeFrameBuilder},
    headers::{HeartBeatIntervalls, HeartBeatValue, ServerValue, StompVersion, StompVersions},
    server::ServerFrame,
};

use super::*;

#[tokio::test]
async fn connect_defaults() {
    assert_client_behaviour(
        send(
            ConnectFrameBuilder::new("here".to_owned(), StompVersions(vec![StompVersion::V1_2]))
                .build(),
        )
        .then(receive(|bytes| match ServerFrame::try_from(bytes) {
            Ok(ServerFrame::Connected(connected)) => {
                connected.version.value() == &StompVersion::V1_2
                    && connected.server.as_ref().map(ServerValue::value).unwrap()
                        == &("little-stomper/".to_owned() + env!("CARGO_PKG_VERSION"))
                    && connected.session.is_none()
                    && connected
                        .heartbeat
                        .as_ref()
                        .map(HeartBeatValue::value)
                        .unwrap()
                        == &HeartBeatIntervalls::default()
            }
            _ => false,
        })),
    )
    .await;
}

#[tokio::test]
async fn unsupported_stomp_version() {
    assert_client_behaviour(unsupported_stomp_version_errors.then(wait_for_disconnect)).await;
}

fn unsupported_stomp_version_errors<'a>(
    in_sender: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    async move {
        let connect =
            ConnectFrameBuilder::new("here".to_owned(), StompVersions(vec![StompVersion::V1_1]))
                .build();

        send_data(in_sender, connect);

        tokio::task::yield_now().await;

        assert_receive(out_receiver, |bytes| match ServerFrame::try_from(bytes) {
            Ok(ServerFrame::Error(_)) => true,
            _ => false,
        });

        
    }
    .boxed()
}

#[tokio::test]
async fn first_message_not_frame() {
    assert_client_behaviour(send("\n").then(expect_error_and_disconnect)).await;
}

pub fn expect_error_and_disconnect<'a>(
    in_sender: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    expect_error.then(wait_for_disconnect)(in_sender, out_receiver)
}

pub fn expect_error<'a>(
    in_sender: &'a mut InSender<StomperError>,
    out_receiver: &'a mut OutReceiver,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    receive(|bytes| matches!(ServerFrame::try_from(bytes), Ok(ServerFrame::Error(_))))(
        in_sender,
        out_receiver,
    )
}

#[tokio::test]
async fn first_message_not_connect() {
    assert_client_behaviour(send(subscribe_frame()).then(expect_error_and_disconnect)).await;
}

fn subscribe_frame() -> SubscribeFrame {
    SubscribeFrameBuilder::new("foo".to_owned(), "MySub".to_owned()).build()
}
