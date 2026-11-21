//! LoginApp, BaseApp and their versioned client messages.

pub mod base;
pub mod client;
pub mod dispatch;
pub mod entity;
pub mod login;
mod outbox;
mod slots;

use std::{fmt, io};

fn io_invalid_data(message: fmt::Arguments<'_>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
