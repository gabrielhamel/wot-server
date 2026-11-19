#![forbid(unsafe_code)]

//! The server's bounded contexts communicate through published entry points.
//!
//! A consumer can use the account revision while the stored account stays
//! private to Accounts.
//!
//! ```
//! use wot_server::accounts::contract::AccountView;
//!
//! fn revision(account: &AccountView) -> i64 {
//!     account.revision()
//! }
//! ```
//!
//! ```compile_fail,E0616
//! use wot_server::accounts::contract::AccountView;
//!
//! fn storage(account: &AccountView) {
//!     let _ = &account.0;
//! }
//! ```
//!
//! The runtime API excludes the concrete catalog and repository ports.
//!
//! ```compile_fail
//! use wot_server::accounts::GarageCatalog;
//! ```
//!
//! ```compile_fail
//! use wot_server::social::application::SocialRepository;
//! ```
//!
//! ```compile_fail
//! use wot_server::player_sessions::mercury;
//! ```
//!
//! Only the server composition root can create the value required to wire
//! PostgreSQL adapters and the game gateway.
//!
//! ```compile_fail,E0451
//! use wot_server::server::Composition;
//! let _ = Composition { _private: () };
//! ```

pub mod accounts;
pub(crate) mod player_administration;
pub mod player_sessions;
pub mod server;
pub(crate) mod shared_kernel;
pub mod social;

#[doc(hidden)]
pub mod platform;
