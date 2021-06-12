use std::{pin::Pin, sync::Arc, time::Duration};

use futures::{
    future::{join, ready},
    Future, FutureExt,
};
use little_stomper::asynchronous::{
    client::ClientSession, destinations::AsyncDestinations, inmemory::InMemDestination,
    mpsc_sink::UnboundedSenderSink,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use tokio_stream::wrappers::UnboundedReceiverStream;

pub type InSender = UnboundedSender<Result<Vec<u8>, little_stomper::error::StomperError>>;
pub type OutReceiver = UnboundedReceiver<Vec<u8>>;
pub trait BehaviourFunction:
    FnOnce(InSender, OutReceiver) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>>
    + Send
{
}

impl<
        T: FnOnce(
                InSender,
                OutReceiver,
            ) -> Pin<Box<dyn Future<Output = (InSender, OutReceiver)> + Send>>
            + Send,
    > BehaviourFunction for T
{
}

pub trait Chainable {
    fn then<S: BehaviourFunction + 'static>(self, followed_by: S) -> Box<dyn BehaviourFunction>;
}

impl<T: BehaviourFunction + 'static> Chainable for T {
    fn then<S: BehaviourFunction + 'static>(self, followed_by: S) -> Box<dyn BehaviourFunction> {
        Box::new(|in_sender, out_receiver| {
            self(in_sender, out_receiver)
                .then(|(in_sender, out_receiver)| followed_by(in_sender, out_receiver))
                .boxed()
        })
    }
}

pub fn send_data<T: std::fmt::Display>(in_sender: &InSender, data: T) {
    in_sender
        .send(Ok(data.to_string().into_bytes()))
        .expect("Connect failed");
}

pub async fn test_client_expectations<T: BehaviourFunction>(client_behaviour: T) {
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

pub fn assert_receive<T: FnOnce(Vec<u8>) -> bool>(
    out_receiver: &mut OutReceiver,
    message_matcher: T,
) {
    let response = out_receiver.recv().now_or_never();

    if let Some(Some(bytes)) = response {
        assert!(message_matcher(bytes))
    } else {
        if response.is_none() {
            panic!("No server message");
        } else {
            panic!("Unexpected server message:{:?}", response.unwrap());
        }
    }
}

pub fn sleep_in_pause(millis: u64) -> impl Future<Output = ()> {
    tokio::time::pause();
    tokio::time::sleep(Duration::from_millis(millis)).inspect(|_| tokio::time::resume())
}
