use std::{convert::TryFrom, sync::Arc};

use futures::{future::join, FutureExt, StreamExt};
use little_stomper::asynchronous::{
    client::ClientSession, destinations::AsyncDestinations, inmemory::InMemDestination,
    mpsc_sink::UnboundedSenderSink,
};
use stomp_parser::{
    client::ConnectFrame,
    headers::{AcceptVersionValue, HostValue, StompVersion, StompVersions},
    server::ServerFrame,
};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;

#[tokio::test]
async fn it_works() {
    let (in_sender, in_receiver) = unbounded_channel();
    let (out_sender, out_receiver) = unbounded_channel();

    let destinations = AsyncDestinations::start(Arc::new(InMemDestination::create)).await;

    let session_future = ClientSession::process_stream(
        Box::pin(UnboundedReceiverStream::new(in_receiver)),
        Box::pin(UnboundedSenderSink::from(out_sender)),
        destinations,
    )
    .boxed();

    let other_future = tokio::task::spawn(async move {
        let connect = ConnectFrame::new(
            HostValue::new("here".to_owned()),
            AcceptVersionValue::new(StompVersions(vec![StompVersion::V1_2])),
            None,
            None,
            None,
        );

        let mut receiver_stream = UnboundedReceiverStream::new(out_receiver);

        in_sender
            .send(Ok(connect.to_string().into_bytes()))
            .expect("Connect failed");

        tokio::task::yield_now().await;

        let response = receiver_stream.next().now_or_never();

        if let Some(Some(bytes)) = response {
            let frame = ServerFrame::try_from(bytes);

            assert!(matches!(frame, Ok(ServerFrame::Connected(_))))
        }
        receiver_stream.close();
    });

    let results = join(session_future, other_future).await;

    assert!(results.0.is_ok());
    assert!(results.1.is_ok());
}
