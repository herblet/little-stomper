use futures::FutureExt;
use std::{
    future::{ready, Future},
    pin::Pin,
};

use crate::error::StomperError;

const DEFAULT_SERVER: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub trait Client: Sync + Send + Clone {
    fn server(&self) -> Option<String> {
        Some(DEFAULT_SERVER.to_owned())
    }

    fn session(&self) -> Option<String> {
        None
    }
}

pub trait ClientFactory<C: Client>: Sync + Send {
    fn create(
        &mut self,
        login: Option<&str>,
        passcode: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<C, StomperError>> + Send + 'static>>;
}

#[derive(Clone)]
pub struct NoopClient {}

impl Client for NoopClient {}

pub struct NoopClientFactory {}

impl ClientFactory<NoopClient> for NoopClientFactory {
    fn create(
        &mut self,
        login: Option<&str>,
        passcode: Option<&str>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<NoopClient, StomperError>> + Send + 'static>>
    {
        ready(Ok(NoopClient {})).boxed()
    }
}
