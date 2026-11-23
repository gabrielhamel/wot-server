fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=WOT_SERVER_VERSION");
    println!("cargo:rerun-if-env-changed=WOT_GAME_VERSION");
    for name in ["WOT_SERVER_VERSION", "WOT_GAME_VERSION"] {
        if let Ok(value) = std::env::var(name) {
            println!("cargo:rustc-env={name}={value}");
        }
    }
    wot_web_build::build(true)?;
    Ok(())
}
