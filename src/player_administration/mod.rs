mod infrastructure;
mod presentation;

pub(crate) use infrastructure::serve;
pub(crate) use presentation::{AdminToken, router};
