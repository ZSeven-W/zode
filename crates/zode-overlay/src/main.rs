//! Overlay helper entrypoint. Reads JSON-lines commands on stdin; exits on
//! `quit` or EOF (parent death). macOS runs the AppKit overlay; every other
//! platform is a stub that exits immediately.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("zode-overlay: unsupported platform (macOS only)");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    zode_overlay::app::run();
}
