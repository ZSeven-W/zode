mod args;

use args::Args;
use clap::Parser;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ZODE_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    // Phase 01 only wires --version (handled by clap) and arg parsing.
    // Real dispatch lands in Phase 02 (headless) and Phase 04 (TUI).
    let _ = &args;
    eprintln!(
        "zode: interactive modes are not implemented until phase 02 (headless) / phase 04 (tui)."
    );
    std::process::exit(1);
}
