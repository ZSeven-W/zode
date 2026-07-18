//! Overlay helper entrypoint. Reads JSON-lines commands on stdin; exits on
//! `quit` or EOF (parent death). macOS gets the AppKit overlay (Task 4);
//! every other platform is a stub that exits immediately.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("zode-overlay: unsupported platform (macOS only)");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    app::run();
}

#[cfg(target_os = "macos")]
mod app {
    use std::io::BufRead;
    use zode_overlay::proto::{parse_line, OverlayCmd};

    /// Task-2 placeholder: headless command loop (no windows yet). Task 4
    /// replaces this with the AppKit overlay and moves stdin to a thread.
    pub fn run() {
        println!("{{\"ready\":true}}");
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if let Some(OverlayCmd::Quit) = parse_line(&line) {
                break;
            }
        }
    }
}
