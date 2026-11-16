#![forbid(unsafe_code)]

fn main() -> anyhow::Result<()> {
    wot_server::binary_mapping::run()
}
