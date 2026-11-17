#![forbid(unsafe_code)]

mod command;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    command::run().await
}
