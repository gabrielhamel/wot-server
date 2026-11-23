#![forbid(unsafe_code)]

fn main() -> anyhow::Result<()> {
    let bundle = wot_web_build::build(false)?;
    println!("Dashboard and Store assets are in {}", bundle.display());
    Ok(())
}
