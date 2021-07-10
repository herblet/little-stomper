use crate::framework::*;

use std::{convert::TryFrom, pin::Pin};

use futures::{Future, FutureExt};
use stomp_parser::{
    client::{ConnectFrameBuilder, SubscribeFrame, SubscribeFrameBuilder},
    headers::{HeartBeatIntervalls, HeartBeatValue, ServerValue, StompVersion, StompVersions},
    server::ServerFrame,
};

#[tokio::test]
async fn connect_defaults() {
    test_client_expectations(
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
    test_client_expectations(unsupported_stomp_version_errors.then(wait_for_disconnect)).await;
}

fn unsupported_stomp_version_errors(
    in_sender: InSender,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>> {
    async move {
        let connect =
            ConnectFrameBuilder::new("here".to_owned(), StompVersions(vec![StompVersion::V1_1]))
                .build();

        send_data(&in_sender, connect);

        tokio::task::yield_now().await;

        assert_receive(&mut out_receiver, |bytes| {
            match ServerFrame::try_from(bytes) {
                Ok(ServerFrame::Error(_)) => true,
                _ => false,
            }
        });

        (in_sender, out_receiver)
    }
    .boxed()
}

#[tokio::test]
async fn first_message_not_frame() {
    test_client_expectations(send("\n").then(expect_error_and_disconnect)).await;
}

#[tokio::test]
async fn first_message_not_connect() {
    test_client_expectations(send(subscribe_frame()).then(expect_error_and_disconnect)).await;
}

fn subscribe_frame() -> SubscribeFrame {
    SubscribeFrameBuilder::new("foo".to_owned(), "MySub".to_owned()).build()
}
