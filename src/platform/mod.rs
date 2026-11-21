mod infrastructure;

pub use infrastructure::client_formats;
pub(crate) use infrastructure::http;
pub(crate) use infrastructure::observability;

#[cfg(feature = "integration-test-support")]
#[doc(hidden)]
pub mod mercury;

#[cfg(not(feature = "integration-test-support"))]
pub(crate) mod mercury;
