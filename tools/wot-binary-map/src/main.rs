#![forbid(unsafe_code)]

mod binary_mapping;

fn main() -> anyhow::Result<()> {
    binary_mapping::run()
}
