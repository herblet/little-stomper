use std::str::{self,FromStr};
use std::convert::TryFrom;
use crate::frame::StompError;

/// A STOMP client command, i.e. one that can be sent by the client
#[allow(non_camel_case_types)]
#[derive(Debug, Eq, PartialEq, EnumString, Clone)]
pub enum ClientCommand {
    CONNECT,
    STOMP,
    SEND,
    SUBSCRIBE,
    UNSUBSCRIBE,
    ACK,
    NACK,
    BEGIN,
    COMMIT,
    ABORT,
    DISCONNECT
}

#[allow(non_camel_case_types)]
#[derive(Debug, Eq, PartialEq, EnumString, Clone)]
pub enum StompVersion {
    #[strum(serialize="1.0")]
    V1_0,
    #[strum(serialize="1.1")]
    V1_1,
    #[strum(serialize="1.2")]
    V1_2

}
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Header {
    AcceptVersion,
    ContentLength,
    ContentType,
    Destination,
    HeartBeat,
    Host,
    Id,
    Login,
    Message,
    MessageId,
    Passcode,
    Receipt,
    ReceiptId,
    Server,
    Session,
    Subscription,
    Transaction,
    Version,
    Custom
}

impl TryFrom<&[u8]> for Header {

    type Error = StompError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let text: &str = str::from_utf8(value).map_err(|error| StompError::with_cause(
            String::from("Error parsing header"),
            error))?;

        Header::try_from(text)
    }
}

impl TryFrom<&str> for Header {

    type Error = StompError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let sep_index = match value.find(':') {
            None => return Err(StompError::new(String::from("Header has no ':'"))),
            Some(index) => {
                index
            },

        };

        Ok(Self::Custom)
    }
}
#[cfg(test)]
mod test {
    use super::*;
    use tungstenite::protocol::Role::Client;

    #[macro_export]
    macro_rules! parse_version {
        ($text: expr) => { StompVersion::from_str($text).unwrap() }
    }

    #[macro_export]
    macro_rules! parse_header {
        ($text: expr) => { Header::try_from($text).unwrap() }
    }

    #[test]
    fn commands_parse_correctly() {
        assert_eq!(ClientCommand::CONNECT, ClientCommand::from_str("CONNECT").unwrap());
    }

    #[test]
    fn versions_parse_correctly() {
        assert_eq!(StompVersion::V1_0, parse_version!("1.0"));
        assert_eq!(StompVersion::V1_1, parse_version!("1.1"));
        assert_eq!(StompVersion::V1_2, parse_version!("1.2"));
    }

    #[test]
    fn headers_parse_correctly() {
        assert_eq!(Header::Custom, parse_header!
        ("content-length: 1234"));
    }

}