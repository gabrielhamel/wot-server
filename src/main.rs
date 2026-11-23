#![forbid(unsafe_code)]

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    wot_server_runtime::run().await
}
