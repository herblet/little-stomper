// use futures_util::StreamExt;
use super::{StompClient, StomperError};
use crate::destinations::{
    BorrowedSender, BorrowedSubscriber, DestinationId, Destinations, InboundMessage,
    OutboundMessage, Sender, Subscriber, SubscriptionId,
};
use futures::FutureExt;
use log::info;
use std::borrow::Borrow;
use std::future::{ready, Future};
use std::pin::Pin;
use std::sync::Arc;
use stomp_parser::model::*;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, error::SendError, UnboundedSender};

pub trait FrameHandler {
    fn handle<T: Destinations>(
        &self,
        frame: ClientFrame,
        destinations: &T,
        client: Arc<dyn StompClient + 'static>,
    ) -> Pin<Box<dyn Future<Output = Result<bool, StomperError>> + Send + 'static>>;
}

pub struct FrameHandlerImpl {}

impl FrameHandler for FrameHandlerImpl {
    fn handle<T: Destinations>(
        &self,
        frame: ClientFrame,
        destinations: &T,
        client: Arc<dyn StompClient + 'static>,
    ) -> Pin<Box<dyn Future<Output = Result<bool, StomperError>> + Send + 'static>> {
        match frame {
            ClientFrame::Connect(frame) => {
                if frame
                    .accepted_versions
                    .value()
                    .contains(&StompVersion::V1_2)
                {
                    client.connect_callback(Ok(()));
                } else {
                    client.connect_callback(Err(StomperError::new(
                        format!(
                            "Unavailable Version {:?} requested.",
                            frame.accepted_versions
                        )
                        .as_str(),
                    )));
                }
                ready(Ok(true)).boxed()
            }

            ClientFrame::Subscribe(frame) => {
                destinations.subscribe(
                    DestinationId(frame.destination.value().clone()),
                    Some(SubscriptionId::from(frame.id.value())),
                    client.clone().into_subscriber(),
                );
                ready(Ok(true)).boxed()
            }

            ClientFrame::Send(frame) => {
                destinations.send(
                    DestinationId(frame.destination.value().clone()),
                    InboundMessage {
                        sender_message_id: None,
                        body: frame.body().unwrap().to_owned(),
                    },
                    client.clone().into_sender(),
                );
                ready(Ok(true)).boxed()
            }

            ClientFrame::Disconnect(_frame) => {
                info!("Client Disconnecting");
                ready(Ok(false)).boxed()
            }
            _ => panic!("Unknown frame"),
        }
    }
}
