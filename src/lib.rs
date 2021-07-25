//! An async Stomp 1.2 library.
//!
//! A sample server using websockets on tokio is in `sample_server`.
#![crate_name = "little_stomper"]
#![warn(clippy::all)]
extern crate log;

pub mod asynchronous;
// Must come before modules using the macros!
mod macros;

pub mod client;
pub mod destinations;
pub mod error;
