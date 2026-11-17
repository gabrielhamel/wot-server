#![forbid(unsafe_code)]

mod application;
mod infrastructure;
mod presentation;

fn main() -> anyhow::Result<()> {
    presentation::run()
}
