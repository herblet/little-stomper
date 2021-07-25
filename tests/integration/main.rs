use std::{pin::Pin, sync::Arc};

use sender_sink::wrappers::UnboundedSenderSink;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use futures::{Future, FutureExt, SinkExt};
use little_stomper::{
    asynchronous::{
        client::ClientSession, destinations::AsyncDestinations, inmemory::InMemDestination,
    },
    client::DefaultClientFactory,
    error::StomperError,
};
use stomp_test_utils::*;

use tokio_stream::wrappers::UnboundedReceiverStream;

mod client_hearbeat;
mod connect;
mod server_heartbeat;

fn create_client_session(
    in_receiver: UnboundedReceiver<Result<Vec<u8>, little_stomper::error::StomperError>>,
    out_sender: UnboundedSender<Vec<u8>>,
) -> Pin<Box<dyn Future<Output = Result<(), little_stomper::error::StomperError>> + Send + 'static>>
{
    AsyncDestinations::start(Arc::new(InMemDestination::create))
        .then(|destinations| {
            ClientSession::process_stream(
                Box::pin(UnboundedReceiverStream::new(in_receiver)),
                Box::pin(UnboundedSenderSink::from(out_sender).sink_err_into()),
                destinations,
                DefaultClientFactory {},
            )
        })
        .boxed()
}

async fn assert_client_behaviour<T: BehaviourFunction<StomperError> + 'static>(behaviour: T) {
    assert_behaviour(create_client_session, behaviour).await;
}
