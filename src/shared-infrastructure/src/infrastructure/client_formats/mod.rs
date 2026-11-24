//! Binary formats shared by the Player Sessions and Accounts infrastructure.

pub mod gettext;
pub mod pickle;
pub mod pxml;
pub(crate) mod util;

pub use util::io::WgWriteExt;
