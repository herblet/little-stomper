use std::convert::{From, Infallible};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use sender_sink::wrappers::SinkError;
use stomp_parser::error::StompParseError;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StomperError {
    pub message: String,
}

impl StomperError {
    pub fn new(message: &str) -> StomperError {
        StomperError {
            message: message.to_owned(),
        }
    }
}

impl From<StompParseError> for StomperError {
    fn from(err: StompParseError) -> Self {
        StomperError {
            message: err.message().to_owned(),
        }
    }
}

impl From<Infallible> for StomperError {
    fn from(_: Infallible) -> Self {
        StomperError {
            message: "Should have been Infallible!".to_owned(),
        }
    }
}

impl From<SinkError> for StomperError {
    fn from(err: SinkError) -> Self {
        StomperError {
            message: format!("Error sending to Sink: {:?}", err),
        }
    }
}

impl Display for StomperError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::result::Result<(), std::fmt::Error> {
        formatter.write_str(&self.message)
    }
}

#[derive(Debug)]
pub struct ErrorWrapper(Box<dyn Error + Send + Sync>);

impl ErrorWrapper {
    pub(crate) fn new<E: Into<Box<dyn Error + Send + Sync>>>(err: E) -> ErrorWrapper {
        ErrorWrapper(err.into())
    }
}

impl Display for ErrorWrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?},", self.0)
    }
}

impl<T> From<T> for ErrorWrapper
where
    T: Error + Send + Sync + 'static,
{
    fn from(err: T) -> Self {
        ErrorWrapper::new(err)
    }
}

#[derive(Debug)]
pub struct UnknownCommandError {
    text: &'static str,
}

impl Display for UnknownCommandError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Unknown command {:?}", self.text)
    }
}

impl Error for UnknownCommandError {}

#[cfg(test)]
mod test {
    use sender_sink::wrappers::SinkError;
    use stomp_parser::error::StompParseError;

    use crate::error::{ErrorWrapper, StomperError};
    use std::convert::Infallible;
    use std::error::Error;
    use std::fmt::{Display, Formatter};
    #[derive(Debug)]
    struct ErrorForTest(u32);

    impl Error for ErrorForTest {}

    impl Display for ErrorForTest {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "ErrorForTest({})", self.0)
        }
    }

    #[test]
    fn wraps_error() {
        let error = ErrorForTest(42);

        let wrapped_error = ErrorWrapper::new(error);

        assert_eq!(42, wrapped_error.0.downcast::<ErrorForTest>().unwrap().0);
    }

    #[test]
    fn from_stomp_parse_error() {
        let src = StompParseError::new("freaky");
        let error: StomperError = src.into();

        assert_eq!("freaky", error.message);
    }

    #[test]
    fn from_sink_error() {
        let src = SinkError::ChannelClosed;
        let error: StomperError = src.into();

        assert!(error.message.contains("Closed"));
    }

    fn never_error() -> Result<(), Infallible> {
        Ok(())
    }

    #[test]
    fn from_infallible() {
        let src = never_error();
        let target = src.map_err(StomperError::from);

        // this is really a compiler test
        assert!(target.is_ok());
    }

    #[test]
    fn error_display() {
        let src = StomperError::new("Hello, world!");

        // this is really a compiler test
        assert_eq!("Hello, world!", src.to_string());
    }
}
