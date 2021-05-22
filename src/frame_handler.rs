// use futures_util::StreamExt;
use crate::client::Client as StompClient;
use crate::destinations::{DestinationId, Destinations, InboundMessage, SubscriptionId};
use crate::error::StomperError;
use futures::FutureExt;
use log::info;
use std::future::{ready, Future};
use std::pin::Pin;
use std::sync::Arc;
use stomp_parser::model::*;

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
            // Code looks the same, but uses a differen type - need to fix macro
            ClientFrame::Stomp(frame) => {
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
            ClientFrame::Unsubscribe(_frame) => {
                todo!()
            }

            ClientFrame::Abort(_frame) => {
                todo!()
            }

            ClientFrame::Ack(_frame) => {
                todo!()
            }

            ClientFrame::Begin(_frame) => {
                todo!()
            }

            ClientFrame::Commit(_frame) => {
                todo!()
            }

            ClientFrame::Nack(_frame) => {
                todo!()
            }
        }
    }
}
