use std::convert::From;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use stomp_parser::StompParseError;

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
    use crate::error::ErrorWrapper;
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
}
