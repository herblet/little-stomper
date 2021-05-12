// use futures_util::StreamExt;
use super::{Destinations, StompClient, StomperError};
use futures::FutureExt;
use log::info;
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
        client: Arc<dyn StompClient>,
    ) -> Pin<Box<dyn Future<Output = Result<bool, StomperError>> + Send + 'static>>;
}

pub struct FrameHandlerImpl {}

impl FrameHandler for FrameHandlerImpl {
    fn handle<T: Destinations>(
        &self,
        frame: ClientFrame,
        destinations: &T,
        client: Arc<dyn StompClient>,
    ) -> Pin<Box<dyn Future<Output = Result<bool, StomperError>> + Send + 'static>> {
        match frame {
            ClientFrame::Connect(frame) => ready(
                if frame
                    .accepted_versions
                    .value()
                    .contains(&StompVersion::V1_2)
                {
                    let result = client.send(ServerFrame::Connected(ConnectedFrame::new(
                        VersionValue::new(StompVersion::V1_2),
                        None,
                        None,
                        None,
                    )));

                    if let Err(error) = result {
                        client.send(ServerFrame::Error(ErrorFrame::from_message(&error.message)));

                        Err(error)
                    } else {
                        Ok(true)
                    }
                } else {
                    client.send(ServerFrame::Error(ErrorFrame::from_message(
                        "Version(s) not supported. Only STOMP 1.2 is available.",
                    )));
                    Err(StomperError {
                        message: format!(
                            "Unavailable Version {:?} requested.",
                            frame.accepted_versions
                        ),
                    })
                },
            )
            .boxed(),

            ClientFrame::Subscribe(frame) => destinations
                .subscribe(frame, client.clone())
                .map(move |res| {
                    res.map_err(|error| {
                        client.send(ServerFrame::Error(ErrorFrame::from_message(&error.message)));
                        error
                    })
                    .map(|_| true)
                })
                .boxed(),

            ClientFrame::Send(frame) => destinations
                .send_message(frame)
                .map(move |res| {
                    res.map_err(|error| {
                        client.send(ServerFrame::Error(ErrorFrame::from_message(&error.message)));
                        error
                    })
                    .map(|_| true)
                })
                .boxed(),

            ClientFrame::Disconnect(frame) => {
                info!("Client Disconnecting");
                ready(Ok(false)).boxed()
            }
            _ => panic!("Unknown frame"),
        }
    }
}
