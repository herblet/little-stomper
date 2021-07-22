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
    test_utils::{test_expectations, BehaviourFunction},
};
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

async fn test_client_expectations<T: BehaviourFunction<StomperError>>(behaviour: T) {
    test_expectations(create_client_session, behaviour).await;
}
