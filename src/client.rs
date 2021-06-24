//! Provides an API for the user of the library to check the authentication information provided by stomp, hold
//! metadata regarding the client and its connection, and pass that metadata to other implementation-specific
//! code, for example [`crate::destinations::Destination`].
use futures::FutureExt;
use std::{
    future::{ready, Future},
    pin::Pin,
};

use crate::error::StomperError;

/// The default value for the `server` header in the [`stomp_parser::server::ConnectedFrame`]
pub const DEFAULT_SERVER: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// A representation the client and the connection between client and STOMP backend.
///
/// The specific type used
/// will be implementation specific, and can therefore hold arbitrary information, which is passed to destinations when communicating
/// with them. It therefore forms the bridge between the underlying transport and the destinations, allowing information to be shared between them.
pub trait Client: Sync + Send + Clone {
    /// Indicates the value to returns for the `server` header in the [`stomp_parser::server::ConnectedFrame`], if any.
    ///
    /// Defaults to [`DEFAULT_SERVER`].
    fn server(&self) -> Option<String> {
        Some(DEFAULT_SERVER.to_owned())
    }

    /// Indicates the value to returns for the `session` header in the [`stomp_parser::server::ConnectedFrame`], if any.
    ///
    /// Defaults to [`None`]
    fn session(&self) -> Option<String> {
        None
    }
}

/// A factory which allows creation of a single instance of its parameter type, which must implement [`Client`].
///
/// It passes the  the `login` and `passcode` headers from the [`stomp_parser::client::ConnectFrame`] so that the library user may use them as appropriate.
///
/// The expected usage pattern is that the library caller creates a factory initialised based on the transport parameters (e.g. auth settings)
/// and passes the factors to `little-stomper` when the a message stream on that transport ist to be handled by it.
///
/// An implementation for functions and closures with the same singature as [`ClientFactory::create`] is provided.
pub trait ClientFactory<C: Client>: Sync + Send {
    /// Returns a future yielding the [`Client`] instance, consuming the factory.
    fn create<'a>(
        self,
        login: Option<&'a str>,
        passcode: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<C, StomperError>> + Send + 'static>>;
}

impl<
        C: Client,
        F: FnOnce(
                Option<&str>,
                Option<&str>,
            )
                -> Pin<Box<dyn Future<Output = Result<C, StomperError>> + Send + 'static>>
            + Sync
            + Send,
    > ClientFactory<C> for F
{
    fn create(
        self,
        login: Option<&str>,
        passcode: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<C, StomperError>> + Send + 'static>> {
        self(login, passcode)
    }
}

/// A [`Client`] instance which does nothing beyond the default implementations on the trait.
#[derive(Clone)]
pub struct DefaultClient;

impl Client for DefaultClient {}

/// The factory for [`DefaultClient`].
pub struct DefaultClientFactory;

impl ClientFactory<DefaultClient> for DefaultClientFactory {
    fn create<'a>(
        self,
        _login: Option<&'a str>,
        _passcode: Option<&'a str>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<DefaultClient, StomperError>> + Send + 'static>>
    {
        ready(Ok(DefaultClient {})).boxed()
    }
}
#[cfg(test)]
mod test {
    use super::*;

    #[derive(Clone, Default)]
    struct TestClient {
        login: Option<String>,
        passcode: Option<String>,
    }

    impl Client for TestClient {}

    #[test]
    fn default_server_is_little_stomper_version() {
        let client = TestClient::default();

        assert_eq!(DEFAULT_SERVER, client.server().unwrap());
    }

    #[test]
    fn default_session_is_none() {
        let client = TestClient::default();

        assert_eq!(None, client.session());
    }

    async fn create_client<C: Client, F: ClientFactory<C>>(factory: F) -> C {
        factory
            .create(Some("log1n"), Some("passc0de"))
            .await
            .expect("Should be ok")
    }

    #[tokio::test]
    async fn closure_works_as_factory() {
        let client = create_client(|login: Option<&str>, passcode: Option<&str>| {
            let login = login.map(str::to_owned);
            let passcode = passcode.map(str::to_owned);

            async { Ok(TestClient { login, passcode }) }.boxed()
        })
        .await;

        assert_eq!("log1n", client.login.unwrap());
        assert_eq!("passc0de", client.passcode.unwrap());
    }

    #[test]
    fn default_client_returns_defaults() {
        let client = DefaultClient;

        assert_eq!(DEFAULT_SERVER, client.server().unwrap());
        assert_eq!(None, client.session());
    }

    #[tokio::test]
    async fn default_client_factory_returns_defaults() {
        let client = create_client(DefaultClientFactory).await;

        assert_eq!(DEFAULT_SERVER, client.server().unwrap());
        assert_eq!(None, client.session());
    }
}
