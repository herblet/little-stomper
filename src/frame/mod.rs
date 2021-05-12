pub mod model;

use std::str;
use std::str::FromStr;
use std::fmt::{Display, Formatter, Debug};
use std::fmt;
use std::error;
use model::*;
use crate::error::ErrorWrapper;
use warp::filters::body::bytes;
use tungstenite::protocol::Role::Client;
use std::ops::{Deref, DerefMut};
use std::error::Error;
use std::convert::From;
use crate::frame::model::ClientCommand::STOMP;
use std::fs::read;

/// Errors created by Stomper, which can have a cause
pub enum StompError {
    /// An error without a cause
    Initial {
        message: String
    },
    // An error with a cause
    Caused {
        message: String,
        cause: Box<dyn std::error::Error>
    }
}

impl StompError {
    pub fn with_cause<T: Into<Box<dyn std::error::Error>>>(message: String, cause: T ) ->
                                                                                        StompError
    {
        StompError::Caused {
            message,
            cause: cause.into()
        }
    }
    pub fn new(message: String) -> StompError {
        StompError::Initial {
            message
       }
    }
}

impl Display for StompError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StompError::Initial {message} => write!(f, "Stomp Error: {}", message),
            StompError::Caused {message, cause} => write!(f, "Stomp Error: {}, cause by {}",
                                                          message, cause)

        }
    }
}

impl Debug for StompError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StompError::Initial {message} => write!(f, "Stomp Error: {}", message),
            StompError::Caused {message, cause} => write!(f, "Stomp Error: {}, cause by {}",
                                                          message, cause)

        }
    }
}



impl error::Error for StompError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            StompError::Initial { message: _message } => None,
            StompError::Caused { message: _message, cause} => Some(cause.as_ref())

        }
    }
}



#[derive(Debug)]
pub struct FrameHeader<'a> {
    text: &'a str
}



#[derive(Debug)]
pub struct Frame {
    // Stores the bytes of the message, because the headers etc are slices of it
    bytes: Vec<u8>,
    client_frame: Box<dyn ClientFrame>
}

trait ClientFrame: Debug {
    fn command(&self) -> ClientCommand;
}

pub fn parse_client_frame(bytes: Vec<u8>) -> Result<Frame, StompError> {
    Frame::new(bytes)
}

fn as_str(bytes: &[u8]) -> Result<&str, StompError> {
    str::from_utf8(bytes).map_err(|err| {
        StompError::with_cause("Command ist not utf-8".to_owned(), err)
    })
}

fn parse_client_command(text: &str) -> Result<ClientCommand, StompError> {
    ClientCommand::from_str(text).map_err(|err| {
        StompError::with_cause("Command not recognised".to_owned(), err)
    })
}

fn read_client_command(bytes: &[u8]) -> Result<(usize, ClientCommand), StompError> {

    let mut byte_iter = bytes.iter().enumerate();

    while let Some((index, byte)) = byte_iter.next() {
        if *byte == ('\n' as u8) {
            let mut section = &bytes[..index];

            match section[..index].last() {
                Some(x) if x == &('\r' as u8) => section = section.split_last().unwrap().1,
                _ => {}
            }

            return as_str(section).and_then( parse_client_command  )
                .map(move |command|(index,command))
        }
    };

    return Err(StompError::new(String::from("No Command found")));
}

impl Frame {
    pub fn new(bytes: Vec<u8>) -> Result<Frame, StompError> {

        let (mut byte_index,command) = read_client_command(bytes.as_slice())?;

        let client_frame: Box<dyn ClientFrame> = match command {
            ClientCommand::CONNECT => {
                Box::new(init_connect_frame(bytes.as_slice
                (),byte_index)?)
            } ,
            _ => unimplemented!()
        };

        Ok(Frame {
            bytes: bytes.to_owned(),
            client_frame
        })
    }

    pub fn command(&self) -> ClientCommand {
        self.client_frame.command()
    }
}

fn init_connect_frame<'a>(bytes:&'a[u8], initial_index: usize) -> Result<ConnectFrame<'static>,
    StompError> {
    let mut accept_version: Option<Vec<StompVersion>> = None;
    let mut heartbeat_requested = 0u32;
    let mut heartbeat_supplied =  0u32;
    let mut login: Option<&str> = None;
    let mut passcode: Option<&str> = None;

    let mut index = initial_index;
    let mut empty_line_found = false;

    while let Some((header_slice, new_index)) = next_slice(bytes, index) {
        if(header_slice.is_empty()) {
            empty_line_found = true;
            break
        }
        index = new_index;
        let header = as_str(header_slice)?

        match header.find(":") {
            Some(colon_index) => {
                let (key, mut value) = header.split_at(colon_index);
                value = &value[1..];


            }
        }

    }

    match accept_version {
        None => Err(StompError::new("No accept-version header supplied".to_owned())),
        Some(accept_version) => Ok(ConnectFrame {
            accept_version,
            heartbeat_requested,
            heartbeat_supplied,
            login,
            passcode
        })
    }

}

fn next_slice(bytes: &[u8], index: usize) -> Option<(&[u8], usize)> {
    let (_, remainder) = bytes.split_at(index);

    let mut byte_iter = remainder.iter().enumerate();


    while let Some((slice_len, byte)) = byte_iter.next() {
        if *byte == ('\n' as u8) {
            let mut section = &remainder[..slice_len];

            match section[..slice_len].last() {
                Some(x) if x == &('\r' as u8) => section = section.split_last().unwrap().1,
                _ => {}
            }

            return Some((section, index+slice_len))
        }
    };
    None
}

#[derive(Debug)]
struct ConnectFrame<'a> {
    accept_version: Vec<StompVersion>,
    heartbeat_requested: u32,
    heartbeat_supplied: u32,
    login: Option<&'a str>,
    passcode: Option<&'a str>
}

impl ClientFrame for ConnectFrame<'_> {
    fn command(&self) -> ClientCommand {
        ClientCommand::CONNECT
    }
}

#[cfg(test)]
mod test {
    use super::model::ClientCommand;
    use crate::frame::read_client_command;
    use warp::Buf;

    #[test]
    fn it_works() {
        let mut frame = crate::frame::Frame::new("CONNECT\nversion:1.2\nheart-beat:10000,\
    10000\n\n\u{0000}".as_bytes().to_owned());

        assert!( matches!(frame.unwrap().command(),  ClientCommand::CONNECT));
        //assert!( frame.header(mod::Header))
    }

    #[test]
    fn read_client_command_works() {
        let result = read_client_command("CONNECT\n".as_bytes());

        assert_eq!((9, ClientCommand::CONNECT), result.unwrap());
    }

    #[test]
    fn read_client_command_accepts_cr() {
        let result = read_client_command("CONNECT\r\n".as_bytes());

        assert_eq!((10, ClientCommand::CONNECT), result.unwrap());
    }
}
// impl <'a> Frame<'a> {
//     pub fn command(&self) -> ClientCommand {
//         return self.command.unwrap()
//     }
//     pub fn new(bytes: Vec<u8>) -> Frame<'static> {
//         Frame {
//             bytes,
//             command: None,
//             headers: Vec::new(),
//             body: None
//         }
//     }
//
//     fn apply_section(&mut self, section: &'a str) -> Result<(), StompError<> > {
//         match self.command {
//             None => self.apply_command(section),
//             Some(_) => self.apply_header(section)
//         }
//     }
//
//
//     fn apply_command(&mut self, text: &'a str) -> Result<(), StompError > {
//         ClientCommand::from_str(text).and_then(|command| {
//             self.command = Some(command);
//             Ok(())
//         }).map_err(|err| StompError::with_cause("Error parsing command".to_string(), Box::new
//             (err)))
//     }
//
//     fn apply_header(&mut self, section: &'a str) -> Result<(), StompError> {
//         self.headers.push( FrameHeader { text: section });
//
//         Ok(())
//     }
//
//     fn apply_body(&mut self, bytes: &'a [u8]) -> Result<(), StompError > {
//         self.body = Some(bytes);
//         Ok(())
//     }
//
//     fn parse(&'a mut self) -> Result<(), StompError> {
//
//         let mut start: usize = 0;
//
//         loop {
//
//             let (mut current, left_over) = {
//                 let mut end = start;
//
//
//                 while end < self.bytes.len() && self.bytes[end] != ('\n' as u8) {
//                     end += 1;
//                 }
//
//                 if end == self.bytes.len() {
//                     break Err(StompError::new("Ended too early".to_string()))
//                 } else {
//                     start = end + 1;
//                     self.bytes.split_at(end)
//                 }
//             };
//
//             //let (mut current, left_over) = Frame::split_at_newline(remainder);
//             // Move the offset, accounting for the newline
//
//             current = Frame::remove_trailing_cr(current);
//
//             if current.len() > 0 {
//                 // it should be a string
//                 match str::from_utf8(current) {
//                     Err(_) => break Err(StompError::new("Error parsing UTF-8".to_string())),
//                     Ok(section) => self.apply_section(section)?
//                 }
//
//                 if left_over.len() == 0 {
//                     break Err(StompError::new("Headers ended without empty line".to_string()))
//                 }
//             } else {
//                 match left_over.split_last() {
//                     None => break Err(StompError::new("Empty body with no null char".to_string())),
//                         // ())),
//                     Some((last_byte, body)) => {
//                         match *last_byte {
//                             0u8 => {
//                                 self.body = Some(body);
//                                 break Ok(())
//                             },
//                             _ => break Err(StompError::new("Last byte must be 0".to_string()))
//                         }
//                     }
//                 }
//             }
//         }
//     }
//
//     pub fn from_bytes(input: Vec<u8>) -> Result<Frame<'a>, StompError> {
//         // The frame takes ownership of the bytes; everything in the frame is within the bytes;
//         // ergo the frame is 'static
//         let mut frame = Frame::new(input);
//         frame.parse();
//         Ok(frame)
//     }
//
//     fn remove_trailing_cr(current: &[u8]) -> &[u8] {
//         if current.len() > 0 &&
//             *current.last().unwrap() == ('\r' as u8) {
//             &current[0..current.len() - 1]
//         } else {
//             current
//         }
//     }
//
//     fn split_at_newline(bytes: &'a[u8]) -> (&'a [u8], Option<&'a [u8]>) {
//         let mut split = bytes.splitn(2,|b| (*b) == '\n' as u8 );
//
//         (split.next().unwrap(),split.next())
//     }
// }
//
// #[cfg(test)]
// mod tests {
//     use crate::frame::model::ClientCommand;
//     use crate::frame::Frame;
//     use std::str;
//
//
//     #[test]
//     fn remove_trailing_cr_removes() {
//         let res = Frame::remove_trailing_cr( "foo\r".as_bytes());
//         assert_eq!("foo", str::from_utf8(res).unwrap());
//     }
//
//     #[test]
//     fn remove_trailing_cr_works_without() {
//         let res = Frame::remove_trailing_cr( "foo\rbar".as_bytes());
//         assert_eq!("foo\rbar", str::from_utf8(res).unwrap());
//     }
//
//     #[test]
//     fn split_at_newline_splits() {
//         let (first,second) = Frame::split_at_newline( "foo\nbar".as_bytes());
//         assert!(second.is_some());
//         assert_eq!("foo", str::from_utf8(first).unwrap());
//         assert_eq!("bar", str::from_utf8(second.unwrap()).unwrap())
//     }
//
//     #[test]
//     fn split_at_newline_splits_last() {
//         let (first,second) = Frame::split_at_newline( "foo\n".as_bytes());
//         assert!(second.is_some());
//         assert!(second.unwrap().is_empty());
//         assert_eq!("foo", str::from_utf8(first).unwrap());
//     }
//
//     #[test]
//     fn split_at_newline_works_with_none() {
//         let (first,second) = Frame::split_at_newline( "foo".as_bytes());
//         assert!(second.is_none());
//         assert_eq!("foo", str::from_utf8(first).unwrap());
//     }
//
//     #[test]
//     fn it_works() {
//         let result = crate::frame::Frame::from_bytes(Vec::from("CONNECT\nversion:1.2\nheart-beat:10000,\
//     10000\n\n\u{0000}".as_bytes()));
//
//         assert!(result.is_ok());
//         let frame = result.unwrap();
//         assert!( matches!(frame.command(),  ClientCommand::CONNECT));
//     }
//
//     #[test]
//     fn it_works_for_stomp_command() {
//         let result = crate::frame::Frame::from_bytes(Vec::from("STOMP\nversion:1.2\nheart-beat:10000,\
//     10000\n\n\u{0000}".as_bytes()));
//
//         assert!(result.is_ok());
//         let frame = result.unwrap();
//         assert!( matches!(frame.command(),  ClientCommand::STOMP));
//     }
//
//     #[test]
//     fn it_accepts_carriage_returns() {
//         let result = crate::frame::Frame::from_bytes(Vec::from("CONNECT\r\nversion:1.2\r\nheart-beat:10000,\
//     10000\n\n\u{0000}".as_bytes()));
//
//         assert!(result.is_ok());
//         let frame = result.unwrap();
//         assert!( matches!(frame.command(),  ClientCommand::CONNECT));
//     }
//
//     #[test]
//     fn it_rejects_unknown_commands() {
//         let result = crate::frame::Frame::from_bytes(Vec::from("UNKNOWN\nversion:1.2\nheart-beat:10000,\
//     10000\n\n\u{0000}".as_bytes()));
//
//         assert!(result.is_err());
//     }
// }
//
//
