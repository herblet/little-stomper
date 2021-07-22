use std::{convert::TryInto, pin::Pin, time::Duration};

use futures::{
    future::{join, ready},
    Future, FutureExt,
};
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::yield_now,
};

pub trait ErrorType: Send + std::fmt::Debug + 'static {}

impl<T: Send + std::fmt::Debug + 'static> ErrorType for T {}

pub type InSender<E> = UnboundedSender<Result<Vec<u8>, E>>;
pub type InReceiver<E> = UnboundedReceiver<Result<Vec<u8>, E>>;

pub type OutReceiver = UnboundedReceiver<Vec<u8>>;
pub type OutSender = UnboundedSender<Vec<u8>>;

pub trait SessionFactory<E: ErrorType>:
    FnOnce(InReceiver<E>, OutSender) -> Pin<Box<dyn Future<Output = Result<(), E>> + Send>>
{
}

impl<
        E: ErrorType,
        F: FnOnce(InReceiver<E>, OutSender) -> Pin<Box<dyn Future<Output = Result<(), E>> + Send>>,
    > SessionFactory<E> for F
{
}

pub trait BehaviourFunction<E: ErrorType>:
    FnOnce(
        InSender<E>,
        OutReceiver,
    ) -> Pin<Box<dyn Future<Output = (InSender<E>, OutReceiver)> + Send>>
    + Send
{
}

impl<
        E: ErrorType,
        T: FnOnce(
                InSender<E>,
                OutReceiver,
            ) -> Pin<Box<dyn Future<Output = (InSender<E>, OutReceiver)> + Send>>
            + Send,
    > BehaviourFunction<E> for T
{
}

pub trait Chainable<E: ErrorType> {
    fn then<S: BehaviourFunction<E> + 'static>(
        self,
        followed_by: S,
    ) -> Box<dyn BehaviourFunction<E>>;
}

impl<E: ErrorType, T: BehaviourFunction<E> + 'static> Chainable<E> for T {
    fn then<S: BehaviourFunction<E> + 'static>(
        self,
        followed_by: S,
    ) -> Box<dyn BehaviourFunction<E>> {
        Box::new(|in_sender, out_receiver| {
            self(in_sender, out_receiver)
                .then(|(in_sender, out_receiver)| followed_by(in_sender, out_receiver))
                .boxed()
        })
    }
}

pub fn send<
    E: ErrorType,
    E2: Into<E> + ErrorType,
    T: TryInto<Vec<u8>, Error = E2> + Send + 'static,
>(
    data: T,
) -> impl BehaviourFunction<E> {
    |in_sender, out_receiver| {
        async {
            send_data(&in_sender, data);
            yield_now().await;
            (in_sender, out_receiver)
        }
        .boxed()
    }
}

pub fn send_data<E: ErrorType, E2: Into<E>, T: TryInto<Vec<u8>, Error = E2>>(
    in_sender: &InSender<E>,
    data: T,
) {
    in_sender
        .send(data.try_into().map_err(|e2| e2.into()))
        .expect("Connect failed");
}

pub async fn test_expectations<E: ErrorType, F: SessionFactory<E>, T: BehaviourFunction<E>>(
    session_factory: F,
    client_behaviour: T,
) {
    let (in_sender, in_receiver) = unbounded_channel::<Result<Vec<u8>, E>>();
    let (out_sender, out_receiver) = unbounded_channel();

    let session_future = session_factory(in_receiver, out_sender);

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

pub fn receive<E: ErrorType, T: FnOnce(Vec<u8>) -> bool + Send + 'static>(
    message_matcher: T,
) -> impl BehaviourFunction<E> {
    |in_sender, mut out_receiver| {
        async {
            assert_receive(&mut out_receiver, message_matcher);
            (in_sender, out_receiver)
        }
        .boxed()
    }
}

pub fn sleep_in_pause(millis: u64) -> impl Future<Output = ()> {
    tokio::time::pause();
    tokio::time::sleep(Duration::from_millis(millis)).inspect(|_| tokio::time::resume())
}

pub fn wait_for_disconnect<E: ErrorType>(
    in_sender: InSender<E>,
    mut out_receiver: OutReceiver,
) -> Pin<Box<dyn Future<Output = (InSender<E>, OutReceiver)> + Send>> {
    async move {
        sleep_in_pause(5050).await;

        assert!(matches!(out_receiver.recv().now_or_never(), Some(None)));
        (in_sender, out_receiver)
    }
    .boxed()
}
