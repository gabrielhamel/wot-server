#![forbid(unsafe_code)]

mod infrastructure;

pub use infrastructure::client_formats;
pub use infrastructure::http;
pub use infrastructure::observability;

pub mod mercury;

#[doc(hidden)]
pub use paste::paste as __paste;
