use std::env;

const WINDOWS_ICON: &str = "../../assets/brand/zode.ico";

fn main() {
    println!("cargo:rerun-if-changed={WINDOWS_ICON}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    winresource::WindowsResource::new()
        .set_icon(WINDOWS_ICON)
        .set("FileDescription", "Zode Desktop")
        .set("ProductName", "Zode")
        .set("OriginalFilename", "zode-app.exe")
        .compile()
        .expect("compile the Zode Windows executable resources");
}
